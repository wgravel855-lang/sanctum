//! Adapter DNS management (ADR-001 §6).
//!
//! - **Enumerate** active adapters via `GetAdaptersAddresses` (FFI): index,
//!   GUID, connection name, up/loopback/type.
//! - **Capture** each adapter's prior DNS + DHCP-vs-static origin by reading
//!   the registry `NameServer` keys (empty ⇒ was DHCP) — the most reliable,
//!   reviewable source, and it never captures our own loopback sinkhole.
//! - **Apply / restore** via `netsh` keyed by the adapter's *current*
//!   connection name (not a hardcoded localized string) and separately for
//!   IPv4 (`127.0.0.1`) and IPv6 (`::1`) — the IPv6 leg is the AAAA-leak fix.
//!
//! Set/restore require elevation and are verified by the elevated ritual in
//! the README, not in unit tests. The command *construction* is pure and is
//! unit-tested here.

use std::os::windows::process::CommandExt;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// `CREATE_NO_WINDOW` — keep netsh/ipconfig from flashing a console when the
/// service (or a test) shells out.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;

pub const LOOPBACK_V4: &str = "127.0.0.1";
pub const LOOPBACK_V6: &str = "::1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpFamily {
    V4,
    V6,
}

impl IpFamily {
    fn netsh(self) -> &'static str {
        match self {
            IpFamily::V4 => "ipv4",
            IpFamily::V6 => "ipv6",
        }
    }
    fn loopback(self) -> &'static str {
        match self {
            IpFamily::V4 => LOOPBACK_V4,
            IpFamily::V6 => LOOPBACK_V6,
        }
    }
}

/// An active network adapter we may repoint.
#[derive(Debug, Clone)]
pub struct Adapter {
    pub index: u32,
    /// Registry GUID, e.g. `{AABBCCDD-...}`.
    pub guid: String,
    /// Connection name (what netsh's `name=` wants), e.g. `Ethernet`.
    pub name: String,
    pub if_type: u32,
    pub is_up: bool,
}

impl Adapter {
    /// Adapters we manage: operationally up and not the loopback pseudo-NIC.
    /// VPN/tunnel/tethering adapters are intentionally kept.
    pub fn is_manageable(&self) -> bool {
        self.is_up && self.if_type != IF_TYPE_SOFTWARE_LOOPBACK
    }
}

/// The prior DNS configuration for one adapter, journaled so we can restore
/// it exactly. An empty family list means that family was DHCP-assigned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterRestore {
    pub name: String,
    pub guid: String,
    pub v4: Vec<String>,
    pub v6: Vec<String>,
}

impl AdapterRestore {
    fn was_dhcp(list: &[String]) -> bool {
        list.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Enumeration (FFI)
// ---------------------------------------------------------------------------

/// Enumerate all adapters via `GetAdaptersAddresses`.
pub fn enumerate() -> anyhow::Result<Vec<Adapter>> {
    use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    let family = AF_UNSPEC.0 as u32;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;

    // 8-byte-aligned scratch buffer (the struct needs >1 alignment).
    let mut size: u32 = 16 * 1024;
    let mut adapters = Vec::new();

    unsafe {
        let mut buf: Vec<u64> = vec![0; (size as usize).div_ceil(8)];
        loop {
            let ret = GetAdaptersAddresses(
                family,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            );
            if ret == ERROR_BUFFER_OVERFLOW.0 {
                buf = vec![0; (size as usize).div_ceil(8)];
                continue;
            }
            if ret != 0 {
                anyhow::bail!("GetAdaptersAddresses failed (code {ret})");
            }
            break;
        }

        let mut cur = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !cur.is_null() {
            let a = &*cur;
            let index = a.Anonymous1.Anonymous.IfIndex;
            let name = a.FriendlyName.to_string().unwrap_or_default();
            let guid = a.AdapterName.to_string().unwrap_or_default();
            adapters.push(Adapter {
                index,
                guid,
                name,
                if_type: a.IfType,
                is_up: a.OperStatus == IfOperStatusUp,
            });
            cur = a.Next;
        }
    }
    Ok(adapters)
}

/// Read an adapter's prior DNS servers from the registry (static list; empty
/// means the family is DHCP-assigned). Our own loopback sinkhole addresses
/// are filtered out so a re-apply never captures `127.0.0.1`/`::1`.
pub fn capture_restore(adapter: &Adapter) -> AdapterRestore {
    let v4 = read_nameserver("Tcpip", &adapter.guid);
    let v6 = read_nameserver("Tcpip6", &adapter.guid);
    AdapterRestore {
        name: adapter.name.clone(),
        guid: adapter.guid.clone(),
        v4,
        v6,
    }
}

fn read_nameserver(stack: &str, guid: &str) -> Vec<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let path = format!(
        r"SYSTEM\CurrentControlSet\Services\{stack}\Parameters\Interfaces\{guid}"
    );
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(key) = hklm.open_subkey(path) else {
        return Vec::new();
    };
    let Ok(raw) = key.get_value::<String, _>("NameServer") else {
        return Vec::new();
    };
    parse_nameserver(&raw)
}

/// Split a registry `NameServer` value and drop our loopback sinkholes.
pub fn parse_nameserver(raw: &str) -> Vec<String> {
    raw.split([',', ' ', '\t'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| *s != LOOPBACK_V4 && *s != LOOPBACK_V6)
        .map(String::from)
        .collect()
}

// ---------------------------------------------------------------------------
// Apply / restore (netsh)
// ---------------------------------------------------------------------------

/// Point one adapter's IPv4 and IPv6 DNS at the loopback sinkhole.
pub fn set_loopback(name: &str) -> anyhow::Result<()> {
    for fam in [IpFamily::V4, IpFamily::V6] {
        run_netsh_batch(&netsh_set_static(fam, name, &[fam.loopback().to_string()]))?;
    }
    Ok(())
}

/// Restore one adapter to its journaled prior DNS (DHCP or static, per family).
pub fn restore(r: &AdapterRestore) -> anyhow::Result<()> {
    let mut cmds = Vec::new();
    for (fam, list) in [(IpFamily::V4, &r.v4), (IpFamily::V6, &r.v6)] {
        if AdapterRestore::was_dhcp(list) {
            cmds.push(netsh_set_dhcp(fam, &r.name));
        } else {
            cmds.extend(netsh_set_static(fam, &r.name, list));
        }
    }
    run_netsh_batch(&cmds)
}

/// Build the netsh argument vectors to set a static DNS server list.
pub fn netsh_set_static(fam: IpFamily, name: &str, servers: &[String]) -> Vec<Vec<String>> {
    let f = fam.netsh();
    let mut cmds = Vec::new();
    let Some((first, rest)) = servers.split_first() else {
        return cmds;
    };
    cmds.push(vec![
        "interface".into(),
        f.into(),
        "set".into(),
        "dnsservers".into(),
        format!("name={name}"),
        "static".into(),
        first.clone(),
        "primary".into(),
        "validate=no".into(),
    ]);
    for (i, s) in rest.iter().enumerate() {
        cmds.push(vec![
            "interface".into(),
            f.into(),
            "add".into(),
            "dnsservers".into(),
            format!("name={name}"),
            format!("address={s}"),
            format!("index={}", i + 2),
            "validate=no".into(),
        ]);
    }
    cmds
}

/// Build the netsh argument vector to revert a family to DHCP-assigned DNS.
pub fn netsh_set_dhcp(fam: IpFamily, name: &str) -> Vec<String> {
    vec![
        "interface".into(),
        fam.netsh().into(),
        "set".into(),
        "dnsservers".into(),
        format!("name={name}"),
        "dhcp".into(),
    ]
}

fn run_netsh_batch(cmds: &[Vec<String>]) -> anyhow::Result<()> {
    for args in cmds {
        let status = Command::new("netsh")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .status()?;
        if !status.success() {
            anyhow::bail!("netsh {:?} exited with {status}", args);
        }
    }
    Ok(())
}

/// Flush the Windows DNS client cache after changing settings.
pub fn flush_dns_cache() -> anyhow::Result<()> {
    let status = Command::new("ipconfig")
        .arg("/flushdns")
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if !status.success() {
        anyhow::bail!("ipconfig /flushdns exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nameserver_parsing_drops_loopback() {
        assert_eq!(
            parse_nameserver("8.8.8.8,1.1.1.1"),
            vec!["8.8.8.8", "1.1.1.1"]
        );
        assert_eq!(parse_nameserver(""), Vec::<String>::new());
        // A re-apply must never capture our own sinkhole as "prior".
        assert_eq!(parse_nameserver("127.0.0.1"), Vec::<String>::new());
        assert_eq!(parse_nameserver("::1 9.9.9.9"), vec!["9.9.9.9"]);
    }

    #[test]
    fn static_command_shape() {
        let cmds = netsh_set_static(IpFamily::V4, "Ethernet", &["127.0.0.1".into()]);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0][0], "interface");
        assert_eq!(cmds[0][1], "ipv4");
        assert!(cmds[0].contains(&"name=Ethernet".to_string()));
        assert!(cmds[0].contains(&"127.0.0.1".to_string()));

        // Multiple servers -> one `set` + N-1 `add`.
        let cmds = netsh_set_static(IpFamily::V6, "Wi-Fi", &["::1".into(), "2606:4700:4700::1111".into()]);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[1][2], "add");
    }

    #[test]
    fn dhcp_command_shape() {
        let cmd = netsh_set_dhcp(IpFamily::V4, "Ethernet");
        assert_eq!(cmd.last().unwrap(), "dhcp");
        assert!(cmd.contains(&"name=Ethernet".to_string()));
    }

    #[test]
    fn restore_journal_roundtrips() {
        let r = AdapterRestore {
            name: "Ethernet".into(),
            guid: "{ABC}".into(),
            v4: vec!["8.8.8.8".into()],
            v6: vec![],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: AdapterRestore = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
        assert!(AdapterRestore::was_dhcp(&back.v6));
        assert!(!AdapterRestore::was_dhcp(&back.v4));
    }
}

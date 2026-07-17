//! Egress hardening (ADR-001 §10).
//!
//! Two layers, split by brick-risk:
//!
//! * **DoH-IP :443 block** — persistent Windows Firewall rules (via `netsh`)
//!   against well-known DoH resolver IPs, TCP **and** UDP (QUIC/HTTP-3).
//!   These can't break plaintext DNS, so they're safe and ON by default.
//!   Visible in `wf.msc` under names prefixed "Sanctum:" — friction, not
//!   stealth.
//!
//! * **Plaintext-53/853 egress lockdown** — a WFP dynamic session that blocks
//!   outbound DNS to every resolver except `sanctum-service.exe`. This closes
//!   the hardcoded-resolver hole but is kernel-level packet filtering that a
//!   wrong permit filter could turn into a self-inflicted DNS outage, so it is
//!   **OFF by default** in v0.1 until verified on a real machine. The dynamic
//!   session auto-removes its filters if the service handle closes/crashes.

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Well-known DoH resolver IPs (they serve DoH on :443). Blocking :443 to
/// these does not affect plaintext DNS we forward to them on :53.
const DOH_IPS: &[&str] = &[
    // Cloudflare
    "1.1.1.1", "1.0.0.1", "2606:4700:4700::1111", "2606:4700:4700::1001",
    // Google
    "8.8.8.8", "8.8.4.4", "2001:4860:4860::8888", "2001:4860:4860::8844",
    // Quad9
    "9.9.9.9", "149.112.112.112", "2620:fe::fe", "2620:fe::9",
    // OpenDNS
    "208.67.222.222", "208.67.220.220",
    // AdGuard
    "94.140.14.14", "94.140.15.15",
    // CleanBrowsing
    "185.228.168.9", "185.228.169.9",
];

const RULE_TCP: &str = "Sanctum: block DoH IPs (TCP 443)";
const RULE_UDP: &str = "Sanctum: block DoH IPs (UDP 443)";

/// Apply the enabled egress protections. Returns a guard whose `Drop` tears
/// down the WFP session (if any); the persistent firewall rules are removed
/// explicitly by `remove()`.
pub fn apply(block_doh_ips: bool, block_plaintext_dns: bool) -> Firewall {
    if block_doh_ips {
        if let Err(e) = apply_doh_ip_rules() {
            tracing::warn!(error = %e, "could not apply DoH-IP firewall rules");
        }
    }

    let mut fw = Firewall { wfp: None };
    if block_plaintext_dns {
        match wfp::lockdown_dns() {
            Ok(engine) => fw.wfp = Some(engine),
            Err(e) => tracing::error!(error = %e, "WFP DNS lockdown failed; leaving plaintext DNS unfiltered by firewall"),
        }
    }
    fw
}

/// Remove the persistent DoH-IP firewall rules (authorized stop / uninstall).
pub fn remove() {
    for name in [RULE_TCP, RULE_UDP] {
        let _ = run_netsh(&[
            "advfirewall".into(),
            "firewall".into(),
            "delete".into(),
            "rule".into(),
            format!("name={name}"),
        ]);
    }
}

fn apply_doh_ip_rules() -> anyhow::Result<()> {
    let ips = DOH_IPS.join(",");
    remove(); // idempotent: clear any stale copy first
    for (name, proto) in [(RULE_TCP, "TCP"), (RULE_UDP, "UDP")] {
        run_netsh(&[
            "advfirewall".into(),
            "firewall".into(),
            "add".into(),
            "rule".into(),
            format!("name={name}"),
            "dir=out".into(),
            "action=block".into(),
            format!("protocol={proto}"),
            "remoteport=443".into(),
            format!("remoteip={ips}"),
        ])?;
    }
    Ok(())
}

fn run_netsh(args: &[String]) -> anyhow::Result<()> {
    let status = Command::new("netsh")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if !status.success() {
        anyhow::bail!("netsh {:?} exited with {status}", args);
    }
    Ok(())
}

/// Holds the WFP dynamic-session engine handle alive for the process's
/// lifetime. Dropping it closes the engine, which auto-deletes the dynamic
/// filters — so a crash can never leave DNS black-holed.
pub struct Firewall {
    wfp: Option<isize>,
}

impl Drop for Firewall {
    fn drop(&mut self) {
        if let Some(engine) = self.wfp.take() {
            wfp::close(engine);
        }
    }
}

// ---------------------------------------------------------------------------
// WFP dynamic-session DNS egress lockdown (OFF by default; see module docs)
// ---------------------------------------------------------------------------

mod wfp {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::WindowsFilteringPlatform::*;

    const RPC_C_AUTHN_WINNT: u32 = 10;
    // Sanctum permit filters outweigh the block filters (higher weight wins).
    const WEIGHT_BLOCK: u8 = 8;
    const WEIGHT_PERMIT: u8 = 10;
    const DNS_PORTS: [u16; 2] = [53, 853];

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Open a dynamic WFP engine and install block/permit filters so only the
    /// current executable may send outbound DNS. Returns the engine handle
    /// (kept alive by the caller). Filters vanish when it is closed.
    pub fn lockdown_dns() -> anyhow::Result<isize> {
        let exe = std::env::current_exe()?;
        let exe_wide = to_wide(&exe.to_string_lossy());

        unsafe {
            // 1. Open a dynamic session engine.
            let mut session = FWPM_SESSION0::default();
            session.flags = FWPM_SESSION_FLAG_DYNAMIC;
            let mut engine = HANDLE::default();
            let rc = FwpmEngineOpen0(
                PCWSTR::null(),
                RPC_C_AUTHN_WINNT,
                None,
                Some(&session),
                &mut engine,
            );
            if rc != 0 {
                anyhow::bail!("FwpmEngineOpen0 failed: {rc}");
            }

            // 2. Resolve the app id for the permit exception.
            let mut app_id: *mut FWP_BYTE_BLOB = std::ptr::null_mut();
            let rc = FwpmGetAppIdFromFileName0(PCWSTR(exe_wide.as_ptr()), &mut app_id);
            if rc != 0 || app_id.is_null() {
                FwpmEngineClose0(engine);
                anyhow::bail!("FwpmGetAppIdFromFileName0 failed: {rc}");
            }

            let layers = [
                FWPM_LAYER_ALE_AUTH_CONNECT_V4,
                FWPM_LAYER_ALE_AUTH_CONNECT_V6,
            ];
            let result = (|| -> anyhow::Result<()> {
                for layer in layers {
                    for port in DNS_PORTS {
                        add_block_port(engine, layer, port)?;
                        add_permit_app(engine, layer, port, app_id)?;
                    }
                }
                Ok(())
            })();

            FwpmFreeMemory0(&mut (app_id as *mut core::ffi::c_void));

            if let Err(e) = result {
                FwpmEngineClose0(engine);
                return Err(e);
            }
            Ok(engine.0 as isize)
        }
    }

    pub fn close(engine: isize) {
        unsafe {
            let _ = FwpmEngineClose0(HANDLE(engine as *mut core::ffi::c_void));
        }
    }

    unsafe fn port_condition(port: u16) -> FWPM_FILTER_CONDITION0 {
        let mut cond = FWPM_FILTER_CONDITION0::default();
        cond.fieldKey = FWPM_CONDITION_IP_REMOTE_PORT;
        cond.matchType = FWP_MATCH_EQUAL;
        cond.conditionValue.r#type = FWP_UINT16;
        cond.conditionValue.Anonymous.uint16 = port;
        cond
    }

    unsafe fn add_block_port(
        engine: HANDLE,
        layer: windows::core::GUID,
        port: u16,
    ) -> anyhow::Result<()> {
        let mut cond = port_condition(port);
        let mut filter = FWPM_FILTER0::default();
        filter.layerKey = layer;
        filter.action.r#type = FWP_ACTION_BLOCK;
        filter.weight.r#type = FWP_UINT8;
        filter.weight.Anonymous.uint8 = WEIGHT_BLOCK;
        filter.numFilterConditions = 1;
        filter.filterCondition = &mut cond;
        let mut id = 0u64;
        let rc = FwpmFilterAdd0(engine, &filter, None, Some(&mut id));
        if rc != 0 {
            anyhow::bail!("FwpmFilterAdd0 (block :{port}) failed: {rc}");
        }
        Ok(())
    }

    unsafe fn add_permit_app(
        engine: HANDLE,
        layer: windows::core::GUID,
        port: u16,
        app_id: *mut FWP_BYTE_BLOB,
    ) -> anyhow::Result<()> {
        let mut conds = [FWPM_FILTER_CONDITION0::default(); 2];
        conds[0] = port_condition(port);
        conds[1].fieldKey = FWPM_CONDITION_ALE_APP_ID;
        conds[1].matchType = FWP_MATCH_EQUAL;
        conds[1].conditionValue.r#type = FWP_BYTE_BLOB_TYPE;
        conds[1].conditionValue.Anonymous.byteBlob = app_id;

        let mut filter = FWPM_FILTER0::default();
        filter.layerKey = layer;
        filter.action.r#type = FWP_ACTION_PERMIT;
        filter.weight.r#type = FWP_UINT8;
        filter.weight.Anonymous.uint8 = WEIGHT_PERMIT;
        filter.numFilterConditions = 2;
        filter.filterCondition = conds.as_mut_ptr();
        let mut id = 0u64;
        let rc = FwpmFilterAdd0(engine, &filter, None, Some(&mut id));
        if rc != 0 {
            anyhow::bail!("FwpmFilterAdd0 (permit :{port}) failed: {rc}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doh_list_is_nonempty_and_valid_shape() {
        assert!(DOH_IPS.len() >= 10);
        // Every entry parses as an IP address.
        for ip in DOH_IPS {
            assert!(ip.parse::<std::net::IpAddr>().is_ok(), "bad IP {ip}");
        }
    }
}

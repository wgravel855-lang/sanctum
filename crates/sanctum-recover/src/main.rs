//! Sanctum recovery tool (ADR-001 §5, §8).
//!
//! The guaranteed, service-independent way back to a working machine. Run it
//! elevated (in Safe Mode if a lock is active): it restores each adapter's
//! prior DNS from the journal (or DHCP if the journal is gone), removes the
//! Sanctum hosts section and firewall rules, flushes the DNS cache, and prints
//! the manual `netsh` commands in case it lacks the rights to finish.
//!
//! This is the honest escape hatch. It never checks or respects the lock —
//! booting Safe Mode and running this is *the* documented way out.

use sanctum_core::{paths, Db};
use sanctum_service::{firewall, hostsfile, netcfg};

fn main() {
    println!("Sanctum recovery — restoring your network and removing Sanctum's changes.\n");

    // 1. Restore adapter DNS from the journal; fall back to DHCP-for-all.
    let restored = restore_from_journal();
    if !restored {
        println!("No restore journal found — setting all active adapters back to automatic (DHCP).");
        dhcp_all();
    }

    // 2. Remove the Sanctum hosts section.
    match hostsfile::remove(&hostsfile::hosts_path()) {
        Ok(()) => println!("Removed the Sanctum block section from the hosts file."),
        Err(e) => eprintln!("Could not edit the hosts file (run elevated): {e}"),
    }

    // 3. Remove firewall rules.
    firewall::remove();
    println!("Removed Sanctum firewall rules.");

    // 4. Flush the DNS cache.
    let _ = netcfg::flush_dns_cache();

    // 5. Print the manual fallback commands.
    print_manual_commands();

    println!(
        "\nDone. If DNS is still broken, run the commands above in an elevated prompt, or set your\n\
         network adapter's DNS to \"Obtain automatically\" in Windows Network settings."
    );
}

fn restore_from_journal() -> bool {
    let Ok(db) = Db::open(paths::db_path()) else {
        return false;
    };
    let Ok(Some(json)) = db.get_kv("dns_restore") else {
        return false;
    };
    let Ok(journal) = serde_json::from_str::<Vec<netcfg::AdapterRestore>>(&json) else {
        return false;
    };
    if journal.is_empty() {
        return false;
    }
    for r in &journal {
        match netcfg::restore(r) {
            Ok(()) => println!("Restored DNS for adapter \"{}\".", r.name),
            Err(e) => eprintln!("Could not restore \"{}\" (run elevated): {e}", r.name),
        }
    }
    true
}

fn dhcp_all() {
    let Ok(adapters) = netcfg::enumerate() else {
        return;
    };
    for a in adapters.iter().filter(|a| a.is_manageable()) {
        let r = netcfg::AdapterRestore {
            name: a.name.clone(),
            guid: a.guid.clone(),
            v4: vec![],
            v6: vec![],
        };
        let _ = netcfg::restore(&r);
        println!("Set adapter \"{}\" back to DHCP DNS.", a.name);
    }
}

fn print_manual_commands() {
    println!("\nManual fallback (run in an elevated Command Prompt):");
    match netcfg::enumerate() {
        Ok(adapters) => {
            for a in adapters.iter().filter(|a| a.is_manageable()) {
                println!("  netsh interface ipv4 set dnsservers name=\"{}\" dhcp", a.name);
                println!("  netsh interface ipv6 set dnsservers name=\"{}\" dhcp", a.name);
            }
        }
        Err(_) => {
            println!("  netsh interface ipv4 set dnsservers name=\"<Your Adapter>\" dhcp");
            println!("  netsh interface ipv6 set dnsservers name=\"<Your Adapter>\" dhcp");
        }
    }
    println!("  ipconfig /flushdns");
}

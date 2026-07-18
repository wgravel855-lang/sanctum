//! The Windows service host (ADR-001 §4/§5) plus install/uninstall.
//!
//! Subcommands (see `main.rs`):
//!   install    register the LocalSystem auto-start service + failure actions
//!   uninstall  stop + delete (refused while a lock is active, unless Safe Mode)
//!   run        SCM entry point (the service dispatcher)
//!   console    run enforcement in the foreground for development (Ctrl+C to stop)
//!
//! While locked, the service drops `SERVICE_ACCEPT_STOP` so the services.msc
//! "Stop" button greys out (honest OS-level friction) and a stop that does slip
//! through leaves the HOSTS floor in place. An authorized (unlocked) stop
//! restores adapter DNS and removes the floor + firewall rules. Filtering
//! survives UI closure and reboot; the watchdog restarts the service if killed.

use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sanctum_core::{paths, Db};

use crate::engine::EnforcementEngine;

pub const SERVICE_NAME: &str = "SanctumService";
pub const SERVICE_DISPLAY: &str = "Sanctum Protection";
pub const WATCHDOG_NAME: &str = "SanctumWatchdog";

const RECONCILE_SECS: u32 = 20;

// ---------------------------------------------------------------------------
// SCM dispatcher
// ---------------------------------------------------------------------------

pub fn run() -> anyhow::Result<()> {
    windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

windows_service::define_windows_service!(ffi_service_main, service_main);

fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_service() {
        tracing::error!(error = %e, "sanctum-service exited with error");
    }
}

fn run_service() -> anyhow::Result<()> {
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

    // Single-instance guard: never let two service processes race for port 53.
    let _singleton = match acquire_singleton() {
        Some(h) => h,
        None => {
            tracing::warn!("another sanctum-service instance is already running — exiting");
            return Ok(());
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_handler = stop.clone();

    let event_handler = move |control| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop | ServiceControl::Preshutdown => {
                stop_for_handler.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    let running = ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::PRESHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    };
    status_handle.set_service_status(running.clone())?;

    // Reflect the locked-stop refusal in the SCM control mask: while locked we
    // drop STOP (services.msc "Stop" greys out) but keep PRESHUTDOWN so reboots
    // still work.
    let base = running.clone();
    let on_lock = move |locked: bool| {
        let controls = if locked {
            ServiceControlAccept::PRESHUTDOWN
        } else {
            ServiceControlAccept::STOP | ServiceControlAccept::PRESHUTDOWN
        };
        let _ = status_handle.set_service_status(ServiceStatus {
            controls_accepted: controls,
            ..base.clone()
        });
    };

    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(enforce(stop, on_lock));
    if let Err(e) = &result {
        tracing::error!(error = %e, "enforcement loop error");
    }

    status_handle.set_service_status(ServiceStatus {
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        ..running
    })?;
    result
}

/// The enforcement lifecycle: bring-up (anti-brick ordering), reconcile loop,
/// authorized teardown. `on_lock` is invoked with the current lock state so
/// the caller can adjust the SCM control mask (locked-stop refusal).
async fn enforce(stop: Arc<AtomicBool>, on_lock: impl Fn(bool)) -> anyhow::Result<()> {
    let engine = EnforcementEngine::new();

    // 1. Capture prior DNS FIRST (before any change).
    let journal = engine.capture_and_journal().unwrap_or_default();
    let upstreams = engine.compute_upstreams(&journal);

    // 2. Build + bind the resolver. Binding is the anti-brick gate.
    let (resolver, mut block) = engine.build_resolver(upstreams)?;

    // Block-moment intervention plumbing (v0.1.5 §A): the resolver emits each
    // sinkholed adult-block host; one consumer task records the block (bumping
    // the lifetime counter) and runs the debounce, arming interventions the UI
    // polls for. A single task keeps DB writes serial.
    let intervention = Arc::new(crate::intervention::InterventionCenter::new());
    {
        let (block_tx, mut block_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        resolver.set_block_sink(block_tx);
        let center = intervention.clone();
        tokio::spawn(async move {
            let db = Db::open(paths::db_path()).ok();
            while let Some(host) = block_rx.recv().await {
                if let Some(db) = &db {
                    let _ = db.record_block(&host, "dns", chrono::Utc::now());
                }
                center.record_block(&host);
            }
        });
    }

    let bound = engine.bind(&resolver).await;

    // Serve the UI over the named pipe (survives UI closure; the UI has no
    // admin rights and reaches the service only through here).
    {
        let handler = Arc::new(crate::ipc::IpcHandler::new(
            resolver.clone(),
            paths::db_path(),
            intervention.clone(),
        ));
        if let Err(e) = crate::ipc::spawn_server(handler, paths::PIPE_NAME.to_string()) {
            tracing::error!(error = %e, "failed to start ipc server");
        }
    }

    // 3. Apply the HOSTS floor for the bind window, then hand blocking to the
    //    resolver. The floor is a DEGRADED-mode fallback: while the resolver is
    //    genuinely serving it handles all blocking AND emits the block events
    //    that drive the counter and interventions — so we LIFT the floor,
    //    because HOSTS would otherwise sinkhole domains before they ever reach
    //    the resolver. Repoint adapters only once the resolver actually answers
    //    (a bound-but-dead socket is exactly what bricked DNS).
    if let Err(e) = engine.apply_floor(&block) {
        tracing::warn!(error = %e, "could not write HOSTS floor");
    }

    let mut serving = bound && engine.self_verify().await;
    if serving {
        engine.remove_hosts_floor().ok();
        let _ = crate::netcfg::flush_dns_cache();
        engine.reassert_loopback().ok();
        tracing::info!("enforcement ACTIVE — resolver serving; HOSTS floor lifted (resolver-primary)");
    } else {
        // Undo any prior repoint so DNS is never pointed at a dead resolver; the
        // HOSTS floor stays to keep blocking while degraded.
        engine.restore_adapters().ok();
        tracing::warn!("enforcement DEGRADED — resolver not answering; HOSTS-only, DNS restored to automatic");
    }

    // Egress hardening tracks the LIVE serving state. It must be dropped if the
    // resolver later dies (otherwise its WFP block would keep the OS resolver
    // from reaching the restored upstreams — a DNS black-hole) and re-armed when
    // the resolver recovers. Hence a mutable guard, reconciled below.
    let (doh_ips, plaintext) = Db::open(paths::db_path())
        .and_then(|db| db.load_config())
        .map(|c| (c.block_doh_ips, c.block_plaintext_dns))
        .unwrap_or((true, false));
    let mut firewall = if serving {
        crate::firewall::apply(doh_ips, plaintext)
    } else {
        crate::firewall::apply(false, false)
    };

    on_lock(is_locked());
    mark_protected_today();

    // 4. Reconcile loop until a stop is requested. Every pass re-verifies the
    //    resolver and keeps adapter DNS consistent with whether it's serving,
    //    so a resolver that dies mid-run can never leave DNS broken.
    let mut ticks = 0u32;
    while !stop.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        on_lock(is_locked());
        ticks += 1;
        if ticks >= RECONCILE_SECS {
            ticks = 0;
            // Re-read the effective blocklist so a protection toggle or list
            // edit propagates to BOTH the resolver and the HOSTS floor (and an
            // emptied list clears the floor), instead of reasserting a stale
            // startup snapshot.
            if let Ok(fresh) = engine.reload(&resolver) {
                block = fresh;
            }

            let now_serving = engine.self_verify().await;
            if now_serving {
                // Serving: the resolver blocks everything and emits urge events.
                // On the recovery transition, lift the HOSTS floor (so it can't
                // short-circuit the resolver), flush stale sinkhole cache, and
                // re-arm egress.
                if !serving {
                    tracing::info!("resolver serving — lifting HOSTS floor (resolver-primary), re-arming egress");
                    engine.remove_hosts_floor().ok();
                    let _ = crate::netcfg::flush_dns_cache();
                    drop(std::mem::replace(
                        &mut firewall,
                        crate::firewall::apply(doh_ips, plaintext),
                    ));
                }
                engine.reassert_loopback().ok();
            } else {
                // Degraded: resolver not answering — fall back to the HOSTS floor
                // (refreshed from the current blocklist).
                engine.apply_floor(&block).ok();
                if serving {
                    tracing::warn!("resolver stopped answering — HOSTS floor re-applied, DNS restored");
                    engine.restore_adapters().ok();
                    // Drop the WFP lockdown so the OS resolver can reach the
                    // restored upstreams instead of being black-holed.
                    drop(std::mem::replace(
                        &mut firewall,
                        crate::firewall::apply(false, false),
                    ));
                }
            }
            serving = now_serving;

            ensure_service_running(WATCHDOG_NAME);
            mark_protected_today();
        }
    }

    // 5. Teardown.
    if is_locked() {
        // A stop while locked is not authorized: keep the HOSTS floor and the
        // firewall rules, but restore adapter DNS so we never leave the machine
        // with a dead resolver (the watchdog will restart us).
        engine.restore_adapters().ok();
        tracing::warn!("stop while locked: HOSTS floor + firewall retained; adapters restored");
    } else {
        engine.restore_adapters().ok();
        engine.remove_hosts_floor().ok();
        crate::firewall::remove();
        tracing::info!("authorized stop: adapters restored, HOSTS floor + firewall removed");
    }
    Ok(())
}

/// Ensure a companion service is RUNNING; start it if it has stopped. Used by
/// the reconcile loop to keep the watchdog alive (mutual supervision).
fn ensure_service_running(name: &str) {
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let Ok(manager) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
    else {
        return;
    };
    let Ok(service) =
        manager.open_service(name, ServiceAccess::QUERY_STATUS | ServiceAccess::START)
    else {
        return; // not installed (e.g. dev console run)
    };
    if let Ok(status) = service.query_status() {
        if !matches!(
            status.current_state,
            ServiceState::Running | ServiceState::StartPending
        ) {
            let _ = service.start(&[] as &[&std::ffi::OsStr]);
            tracing::info!("restarted companion service {name}");
        }
    }
}

fn mark_protected_today() {
    if let Ok(db) = Db::open(paths::db_path()) {
        let _ = db.mark_protected_today();
    }
}

/// Acquire the process-wide single-instance mutex, retrying briefly so a
/// restart can wait for a dying prior process to release it. Returns the handle
/// to hold for the process lifetime (the OS frees it on exit), or `None` if
/// another instance still holds it — preventing two processes racing for :53.
fn acquire_singleton() -> Option<windows::Win32::Foundation::HANDLE> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Global\\SanctumServiceSingleton\0".encode_utf16().collect();
    for _ in 0..10 {
        unsafe {
            match CreateMutexW(None, false, PCWSTR(name.as_ptr())) {
                Ok(h) => {
                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        let _ = CloseHandle(h);
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        continue;
                    }
                    return Some(h);
                }
                Err(_) => return None,
            }
        }
    }
    None
}

fn is_locked() -> bool {
    Db::open(paths::db_path())
        .ok()
        .and_then(|db| db.load_lock().ok())
        .map(|l| l.is_active(Utc::now()))
        .unwrap_or(false)
}

/// True if Windows booted into Safe Mode — the guaranteed teardown path where
/// nothing of ours runs and the lock is unconditionally bypassable.
pub fn in_safe_mode() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CLEANBOOT};
    unsafe { GetSystemMetrics(SM_CLEANBOOT) != 0 }
}

// ---------------------------------------------------------------------------
// Foreground / development
// ---------------------------------------------------------------------------

/// Run enforcement in the foreground until Ctrl+C. For development on an
/// elevated console; behaves like the service without SCM.
pub fn console() -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopper = stop.clone();
    rt.block_on(async move {
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("Ctrl+C — shutting down");
            stopper.store(true, Ordering::SeqCst);
        });
        enforce(stop, |_| {}).await
    })
}

// ---------------------------------------------------------------------------
// Install / uninstall
// ---------------------------------------------------------------------------

use windows_service::service_manager::ServiceManager;

fn register_service(
    manager: &ServiceManager,
    name: &str,
    display: &str,
    exe: std::path::PathBuf,
    description: &str,
) -> anyhow::Result<()> {
    use windows_service::service::{
        ServiceAccess, ServiceAction, ServiceActionType, ServiceErrorControl, ServiceFailureActions,
        ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceType,
    };

    let info = ServiceInfo {
        name: OsString::from(name),
        display_name: OsString::from(display),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec![OsString::from("run")],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };

    let service = manager.create_service(
        &info,
        ServiceAccess::CHANGE_CONFIG | ServiceAccess::START | ServiceAccess::QUERY_STATUS,
    )?;
    service.set_description(description)?;

    // SCM failure actions: restart quickly, even on clean non-zero exits.
    service.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86_400)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![
            ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(1) },
            ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(5) },
            ServiceAction { action_type: ServiceActionType::Restart, delay: Duration::from_secs(30) },
        ]),
    })?;
    service.set_failure_actions_on_non_crash_failures(true)?;
    service.start(&[] as &[&std::ffi::OsStr])?;
    Ok(())
}

/// Wait (up to ~10 s) for a service to reach the Stopped state, so its binary
/// unlocks and the freshly-installed copy can run.
fn wait_for_stopped(service: &windows_service::service::Service) {
    use windows_service::service::ServiceState;
    for _ in 0..50 {
        match service.query_status() {
            Ok(s) if s.current_state == ServiceState::Stopped => return,
            Ok(_) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => return,
        }
    }
}

/// Register a service, or — if a previous install already registered it (same
/// install dir, same exe path) — stop it and start the freshly-installed
/// binary instead of failing on create-already-exists. Retries to ride out the
/// brief "marked for deletion" window a just-run previous uninstaller leaves.
fn ensure_service(
    manager: &ServiceManager,
    name: &str,
    display: &str,
    exe: std::path::PathBuf,
    description: &str,
) -> anyhow::Result<()> {
    use windows_service::service::{ServiceAccess, ServiceState};

    let access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::STOP
        | ServiceAccess::START
        | ServiceAccess::CHANGE_CONFIG;

    let mut last_err = None;
    for _ in 0..30 {
        match manager.open_service(name, access) {
            Ok(existing) => {
                // Already registered (same install dir → same exe path). Stop
                // the old process so the freshly-copied binary is what runs,
                // then start it again.
                let _ = existing.stop();
                wait_for_stopped(&existing);
                let _ = existing.set_description(description);
                let started = existing.start(&[] as &[&std::ffi::OsStr]).is_ok();
                // `start` fails both when already running (benign) and when the
                // service is "marked for deletion" (fatal — the new binary never
                // runs). Distinguish by whether it is actually up.
                let running = matches!(
                    existing.query_status().map(|s| s.current_state),
                    Ok(ServiceState::Running) | Ok(ServiceState::StartPending)
                );
                if started || running {
                    return Ok(());
                }
                // Registered but won't start — typically a just-run previous
                // uninstaller left it pending deletion. Wait for that to clear;
                // a later pass re-opens (still deleting) or, once the record is
                // gone, falls through to the create path below.
                last_err = Some(anyhow::anyhow!(
                    "service {name} is registered but did not start (pending deletion?)"
                ));
                std::thread::sleep(Duration::from_millis(1000));
            }
            // Not present, or the record already cleared: create, then retry if
            // the name is momentarily still reserved.
            Err(_) => match register_service(manager, name, display, exe.clone(), description) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(Duration::from_millis(1000));
                }
            },
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("could not register service {name}")))
}

pub fn install() -> anyhow::Result<()> {
    use windows_service::service_manager::ServiceManagerAccess;

    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )?;

    let exe = std::env::current_exe()?;
    ensure_service(
        &manager,
        SERVICE_NAME,
        SERVICE_DISPLAY,
        exe.clone(),
        "Keeps Sanctum's adult-content filtering active for every browser, even when the app is closed. Visible by design — this is friction, not stealth.",
    )?;

    // Companion watchdog (same install directory).
    match exe.parent().map(|d| d.join("sanctum-watchdog.exe")) {
        Some(w) if w.exists() => ensure_service(
            &manager,
            WATCHDOG_NAME,
            "Sanctum Watchdog",
            w,
            "Restarts Sanctum protection if it stops. Visible by design — friction, not stealth.",
        )?,
        _ => tracing::warn!(
            "sanctum-watchdog.exe not found next to the service; watchdog not installed"
        ),
    }

    tracing::info!("installed and started Sanctum services");
    Ok(())
}

pub fn uninstall() -> anyhow::Result<()> {
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::ServiceManagerAccess;

    if is_locked() && !in_safe_mode() {
        anyhow::bail!(
            "A locked Sanctum session is active. It can't be uninstalled until the timer ends. \
             To remove it sooner, reboot Windows into Safe Mode — that friction is the point."
        );
    }

    // Take enforcement down cleanly before removing the services.
    let engine = EnforcementEngine::new();
    engine.restore_adapters().ok();
    engine.remove_hosts_floor().ok();
    crate::firewall::remove();

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    // Remove the watchdog FIRST so it can't restart the service mid-teardown.
    for name in [WATCHDOG_NAME, SERVICE_NAME] {
        if let Ok(service) = manager.open_service(
            name,
            ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
        ) {
            let _ = service.stop();
            let _ = service.delete();
        }
    }
    tracing::info!("uninstalled Sanctum services");
    Ok(())
}

//! Sanctum watchdog (ADR-001 §4).
//!
//! A second, deliberately VISIBLE LocalSystem service. Every few seconds it
//! checks the main `SanctumService`: if it has stopped (e.g. it was killed),
//! the watchdog starts it again — this is what makes "killing the service
//! doesn't stop filtering for long" true. It also runs an end-to-end DNS
//! liveness canary against the loopback resolver to spot a hung-but-running
//! service. Friction, not stealth: it appears in services.msc and Task Manager
//! and hides nothing.
//!
//! Usage: `sanctum-watchdog run` (SCM) | `sanctum-watchdog console` (dev).

use std::ffi::OsString;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const WATCHDOG_NAME: &str = "SanctumWatchdog";
const TARGET_SERVICE: &str = "SanctumService";
const POLL: Duration = Duration::from_secs(5);

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    match std::env::args().nth(1).as_deref() {
        Some("console") => console(),
        _ => run(), // "run" or SCM-invoked
    }
}

fn run() -> anyhow::Result<()> {
    windows_service::service_dispatcher::start(WATCHDOG_NAME, ffi_service_main)?;
    Ok(())
}

windows_service::define_windows_service!(ffi_service_main, service_main);

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        tracing::error!(error = %e, "watchdog exited with error");
    }
}

fn run_service() -> anyhow::Result<()> {
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

    let stop = Arc::new(AtomicBool::new(false));
    let stop_h = stop.clone();
    let handler = move |control| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop | ServiceControl::Preshutdown => {
                stop_h.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };
    let status_handle = service_control_handler::register(WATCHDOG_NAME, handler)?;

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

    supervise(&stop);

    status_handle.set_service_status(ServiceStatus {
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        ..running
    })?;
    Ok(())
}

fn console() -> anyhow::Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    tracing::info!("watchdog console mode — Ctrl+C to stop");
    // No async runtime; a simple Ctrl+C handler flips the flag.
    let stop_h = stop.clone();
    ctrlc_lite(move || stop_h.store(true, Ordering::SeqCst));
    supervise(&stop);
    Ok(())
}

/// The supervision loop.
fn supervise(stop: &AtomicBool) {
    let mut canary_fail = 0u32;
    while !stop.load(Ordering::SeqCst) {
        sleep_interruptible(POLL, stop);
        if stop.load(Ordering::SeqCst) {
            break;
        }

        match ensure_target_running() {
            Ok(true) => {
                // Running: check end-to-end liveness.
                if canary_ok() {
                    canary_fail = 0;
                } else {
                    canary_fail += 1;
                    if canary_fail >= 3 {
                        tracing::warn!(
                            "{TARGET_SERVICE} is running but its DNS canary has failed {canary_fail} times — it may be hung"
                        );
                    }
                }
            }
            Ok(false) => {
                canary_fail = 0;
                tracing::info!("{TARGET_SERVICE} was not running — started it");
            }
            Err(e) => tracing::debug!(error = %e, "watchdog could not query {TARGET_SERVICE}"),
        }
    }
}

/// Ensure the target service is RUNNING. Returns `Ok(true)` if it was already
/// running, `Ok(false)` if it had to be (re)started.
fn ensure_target_running() -> anyhow::Result<bool> {
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service =
        manager.open_service(TARGET_SERVICE, ServiceAccess::QUERY_STATUS | ServiceAccess::START)?;
    let state = service.query_status()?.current_state;
    if matches!(state, ServiceState::Running | ServiceState::StartPending) {
        Ok(true)
    } else {
        service.start(&[] as &[&std::ffi::OsStr])?;
        Ok(false)
    }
}

/// End-to-end liveness: query the loopback resolver for the health canary and
/// require the fixed `127.0.0.2` answer.
fn canary_ok() -> bool {
    let Ok(sock) = UdpSocket::bind("127.0.0.1:0") else {
        return false;
    };
    let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
    if sock.send_to(&build_canary_query(), "127.0.0.1:53").is_err() {
        return false;
    }
    let mut buf = [0u8; 512];
    match sock.recv(&mut buf) {
        // The A record answer for health.sanctum.invalid is 127.0.0.2.
        Ok(n) => buf[..n].windows(4).any(|w| w == [127, 0, 0, 2]),
        Err(_) => false,
    }
}

fn build_canary_query() -> Vec<u8> {
    // Header: id=0x5741, RD=1, QDCOUNT=1.
    let mut q = vec![
        0x57, 0x41, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    for label in ["health", "sanctum", "invalid"] {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0); // root label
    q.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]); // QTYPE=A, QCLASS=IN
    q
}

fn sleep_interruptible(total: Duration, stop: &AtomicBool) {
    let step = Duration::from_millis(200);
    let mut elapsed = Duration::ZERO;
    while elapsed < total && !stop.load(Ordering::SeqCst) {
        std::thread::sleep(step);
        elapsed += step;
    }
}

/// Minimal Ctrl+C handler for console mode (no external crate).
fn ctrlc_lite(mut on_stop: impl FnMut() + Send + 'static) {
    std::thread::spawn(move || {
        // Block on stdin EOF as a crude "until interrupted" for dev use.
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
        on_stop();
    });
}

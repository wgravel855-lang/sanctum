//! Sanctum service entry point.
//!
//! Usage:
//!   sanctum-service run        (invoked by the Service Control Manager)
//!   sanctum-service install    register the LocalSystem auto-start service
//!   sanctum-service uninstall  remove it (refused while a lock is active)
//!   sanctum-service console    run enforcement in the foreground (dev; Ctrl+C)

use sanctum_service::service;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cmd = std::env::args().nth(1).unwrap_or_else(|| "run".into());
    match cmd.as_str() {
        "run" => service::run(),
        "install" => service::install(),
        "uninstall" => match service::uninstall(
            std::env::args().any(|a| a == "--no-cooldown"),
        )? {
            service::UninstallOutcome::Removed => Ok(()),
            service::UninstallOutcome::Refused { code, message } => {
                // Authoritative, user-facing reason on stdout; the NSIS
                // uninstaller branches on the exit code to show the right box.
                println!("{message}");
                std::process::exit(code);
            }
        },
        "console" => service::console(),
        other => {
            eprintln!("sanctum-service: unknown command {other:?}");
            eprintln!("usage: sanctum-service [run|install|uninstall|console]");
            std::process::exit(2);
        }
    }
}

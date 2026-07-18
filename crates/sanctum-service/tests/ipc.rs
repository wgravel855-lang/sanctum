//! Hermetic test of the IPC server + lock invariants (no admin, temp DB,
//! per-test private pipe). Proves the server-side policy: grow-only blocklist
//! while locked, password gating, and the activity-log wipe.

use std::sync::Arc;
use std::time::Duration;

use sanctum_core::ipc::{Command, Response};
use sanctum_core::{Blocklist, Db, SafeSearchMap};
use sanctum_service::dns::Resolver;
use sanctum_service::ipc::{send, spawn_server, IpcHandler};
use tempfile::TempDir;

/// Start a private IPC server backed by a fresh temp DB. Keeps the TempDir
/// alive by returning it.
async fn start(pipe: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("sanctum.db");
    Db::open(&db_path).unwrap(); // create + migrate

    let resolver = Arc::new(Resolver::new(
        Blocklist::new(),
        SafeSearchMap::new(),
        Blocklist::new(),
        vec![],
    ));
    let intervention = Arc::new(sanctum_service::intervention::InterventionCenter::new());
    let handler = Arc::new(IpcHandler::new(resolver, db_path, intervention));
    spawn_server(handler, pipe.to_string()).unwrap();
    dir
}

/// Send a command, retrying while the pipe is still being stood up.
async fn call(pipe: &str, cmd: Command) -> Response {
    let mut last = String::new();
    for _ in 0..100 {
        match send(pipe, &cmd).await {
            Ok(r) => return r,
            Err(e) => {
                last = e.to_string();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
    panic!("pipe {pipe} never became available: last error = {last}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_block_reflects_in_status() {
    let pipe = r"\\.\pipe\sanctum-test-ipc-status";
    let _dir = start(pipe).await;

    assert!(matches!(
        call(pipe, Command::AddBlock { domain: "example-adult.com".into() }).await,
        Response::Ok
    ));

    match call(pipe, Command::GetStatus).await {
        Response::Status(s) => {
            // starter list (embedded) + our one custom domain.
            assert!(s.blocklist_count > 1, "count was {}", s.blocklist_count);
            assert!(!s.locked);
            assert!(s.all_browsers);
        }
        other => panic!("expected Status, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn intervention_trigger_and_poll() {
    let pipe = r"\\.\pipe\sanctum-test-ipc-intervene";
    let _dir = start(pipe).await;

    // No urge yet.
    match call(pipe, Command::PollIntervention).await {
        Response::Intervention(dto) => assert!(!dto.pending),
        other => panic!("expected Intervention, got {other:?}"),
    }

    // "I need help now" / panic hotkey arms one unconditionally.
    assert!(matches!(
        call(pipe, Command::TriggerIntervention).await,
        Response::Ok
    ));

    // The very next poll fires the window once...
    match call(pipe, Command::PollIntervention).await {
        Response::Intervention(dto) => assert!(dto.pending),
        other => panic!("expected pending Intervention, got {other:?}"),
    }
    // ...and is then cleared (no repeat pop).
    match call(pipe, Command::PollIntervention).await {
        Response::Intervention(dto) => assert!(!dto.pending),
        other => panic!("expected cleared Intervention, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_invariants_are_enforced_server_side() {
    let pipe = r"\\.\pipe\sanctum-test-ipc-lock";
    let _dir = start(pipe).await;

    assert!(matches!(
        call(pipe, Command::StartLock { minutes: 60 }).await,
        Response::Ok
    ));

    // Now locked.
    match call(pipe, Command::GetStatus).await {
        Response::Status(s) => assert!(s.locked && s.locked_until.is_some()),
        other => panic!("expected Status, got {other:?}"),
    }

    // Grow-only: adding is allowed, removing is refused.
    assert!(matches!(
        call(pipe, Command::AddBlock { domain: "newbad.com".into() }).await,
        Response::Ok
    ));
    assert!(matches!(
        call(
            pipe,
            Command::RemoveBlock { domain: "newbad.com".into(), password: String::new() }
        )
        .await,
        Response::Denied { .. }
    ));

    // Timer can be extended but not started again or shortened.
    assert!(matches!(
        call(pipe, Command::ExtendLock { minutes: 30 }).await,
        Response::Ok
    ));
    assert!(matches!(
        call(pipe, Command::StartLock { minutes: 5 }).await,
        Response::Denied { .. }
    ));

    // Protection can't be disabled while locked.
    assert!(matches!(
        call(pipe, Command::DisableProtection { password: String::new() }).await,
        Response::Denied { .. }
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn password_gates_weakening_and_history_wipes() {
    let pipe = r"\\.\pipe\sanctum-test-ipc-pw";
    let _dir = start(pipe).await;

    // Set a password (records an event too via AddBlock below).
    assert!(matches!(
        call(pipe, Command::SetPassword { new: "hunter2".into(), current: None }).await,
        Response::Ok
    ));

    // Add then try to remove with a wrong / right password.
    assert!(matches!(
        call(pipe, Command::AddBlock { domain: "z2.com".into() }).await,
        Response::Ok
    ));
    assert!(matches!(
        call(
            pipe,
            Command::RemoveBlock { domain: "z2.com".into(), password: "wrong".into() }
        )
        .await,
        Response::Denied { .. }
    ));
    assert!(matches!(
        call(
            pipe,
            Command::RemoveBlock { domain: "z2.com".into(), password: "hunter2".into() }
        )
        .await,
        Response::Ok
    ));

    // History wipe returns a count and clears the log.
    match call(pipe, Command::DeleteHistory).await {
        Response::Deleted { count } => assert!(count >= 1),
        other => panic!("expected Deleted, got {other:?}"),
    }
    match call(pipe, Command::RecentEvents { limit: 10 }).await {
        Response::Events(ev) => assert!(ev.is_empty()),
        other => panic!("expected Events, got {other:?}"),
    }
}

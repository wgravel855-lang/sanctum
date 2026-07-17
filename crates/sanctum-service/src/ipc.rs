//! Named-pipe IPC server + command handler (ADR-001 §2, §8).
//!
//! The service is the sole authority: every mutating command is validated
//! here, server-side, against the lock invariants — the client is never
//! trusted. Weakening operations are password-gated (Argon2id). Transport is
//! length-prefixed JSON over a local named pipe (`reject_remote_clients`,
//! `first_pipe_instance`).

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

use sanctum_core::config::{self, LockState};
use sanctum_core::ipc::{Command, EventDto, Response, Status};
use sanctum_core::{ipc as proto, Db};

use crate::dns::Resolver;
use crate::engine;

const MAX_FRAME: usize = 1 << 20; // 1 MiB safety cap

/// Handles decoded commands against the database + running resolver.
pub struct IpcHandler {
    resolver: Arc<Resolver>,
    db_path: PathBuf,
}

impl IpcHandler {
    pub fn new(resolver: Arc<Resolver>, db_path: PathBuf) -> Self {
        Self { resolver, db_path }
    }

    fn db(&self) -> anyhow::Result<Db> {
        Ok(Db::open(&self.db_path)?)
    }

    fn reload(&self, db: &Db) -> anyhow::Result<()> {
        engine::reload_resolver(db, &self.resolver)?;
        Ok(())
    }

    /// Process one command, never panicking: operational failures become
    /// `Response::Error`, policy refusals `Response::Denied`.
    pub fn handle(&self, cmd: Command) -> Response {
        match self.dispatch(cmd) {
            Ok(r) => r,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        }
    }

    fn dispatch(&self, cmd: Command) -> anyhow::Result<Response> {
        let db = self.db()?;
        let now = Utc::now();
        let lock = db.load_lock()?;
        let locked = lock.is_active(now);

        Ok(match cmd {
            Command::GetStatus => Response::Status(self.status(&db, &lock, locked)?),

            Command::RecentEvents { limit } => {
                let events = db
                    .recent_events(limit)?
                    .into_iter()
                    .map(|e| EventDto {
                        ts: e.ts,
                        kind: e.kind,
                        detail: e.detail,
                    })
                    .collect();
                Response::Events(events)
            }

            // Grow-only: always allowed, even while locked.
            Command::AddBlock { domain } => {
                db.add_custom_block(&domain, now)?;
                self.reload(&db)?;
                db.record_event("block_add", &domain, now)?;
                Response::Ok
            }

            Command::RemoveBlock { domain, password } => {
                if locked {
                    return Ok(denied("The block list can only grow during a locked session."));
                }
                if !check_password(&db, &password)? {
                    return Ok(denied("Incorrect password."));
                }
                db.remove_custom_block(&domain)?;
                self.reload(&db)?;
                Response::Ok
            }

            // Can't whitelist your way out of a locked session.
            Command::AddAllow { domain, password } => {
                if locked {
                    return Ok(denied("The allowlist is frozen during a locked session."));
                }
                if !check_password(&db, &password)? {
                    return Ok(denied("Incorrect password."));
                }
                db.add_allow(&domain, now)?;
                self.reload(&db)?;
                Response::Ok
            }

            Command::RemoveAllow { domain } => {
                db.remove_allow(&domain)?;
                self.reload(&db)?;
                Response::Ok
            }

            Command::SetSchedule { schedule, password } => {
                let current = db.load_config()?;
                let mut proposed = current.clone();
                proposed.schedule = schedule;
                if let Err(e) = config::guard_config_change(&current, &proposed, &lock, now) {
                    return Ok(denied(&e.to_string()));
                }
                if !check_password(&db, &password)? {
                    return Ok(denied("Incorrect password."));
                }
                db.save_config(&proposed)?;
                Response::Ok
            }

            Command::StartLock { minutes } => {
                if locked {
                    return Ok(denied("A locked session is already active."));
                }
                if minutes <= 0 {
                    return Ok(denied("Lock duration must be positive."));
                }
                let proposed = LockState::locked_until(now + Duration::minutes(minutes));
                if let Err(e) = config::guard_lock_change(&lock, &proposed, now) {
                    return Ok(denied(&e.to_string()));
                }
                db.save_lock(&proposed)?;
                db.record_event("lock_start", &format!("{minutes} min"), now)?;
                Response::Ok
            }

            Command::ExtendLock { minutes } => {
                if minutes <= 0 {
                    return Ok(denied("Extension must be positive."));
                }
                let base = lock.locked_until.unwrap_or(now).max(now);
                let proposed = LockState::locked_until(base + Duration::minutes(minutes));
                if let Err(e) = config::guard_lock_change(&lock, &proposed, now) {
                    return Ok(denied(&e.to_string()));
                }
                db.save_lock(&proposed)?;
                db.record_event("lock_extend", &format!("+{minutes} min"), now)?;
                Response::Ok
            }

            Command::SetPassword { new, current } => {
                if locked {
                    return Ok(denied("The password is frozen during a locked session."));
                }
                if db.has_password()? {
                    match current {
                        Some(c) if db.verify_password(&c)? => {}
                        _ => return Ok(denied("Incorrect current password.")),
                    }
                }
                db.set_password(&new)?;
                Response::Ok
            }

            Command::VerifyPassword { password } => {
                if check_password(&db, &password)? {
                    Response::Ok
                } else {
                    denied("Incorrect password.")
                }
            }

            Command::DisableProtection { password } => {
                if locked {
                    return Ok(denied(
                        "Protection can't be disabled during a locked session.",
                    ));
                }
                if !check_password(&db, &password)? {
                    return Ok(denied("Incorrect password."));
                }
                let mut cfg = db.load_config()?;
                cfg.protection_enabled = false;
                db.save_config(&cfg)?;
                db.record_event("protection_disabled", "", now)?;
                Response::Ok
            }

            Command::EnableProtection => {
                let mut cfg = db.load_config()?;
                cfg.protection_enabled = true;
                db.save_config(&cfg)?;
                db.record_event("protection_enabled", "", now)?;
                Response::Ok
            }

            Command::DeleteHistory => {
                let count = db.delete_all_history()?;
                Response::Deleted { count }
            }
        })
    }

    fn status(&self, db: &Db, lock: &LockState, locked: bool) -> anyhow::Result<Status> {
        let cfg = db.load_config()?;
        Ok(Status {
            protection_active: cfg.protection_enabled,
            degraded: false,
            total_blocked: db.total_blocks()?,
            protected_days: db.total_protected_days()?,
            streak: db.current_streak()?,
            locked,
            locked_until: lock.locked_until,
            schedule: cfg.schedule,
            blocklist_count: self.resolver.blocklist_len(),
            has_password: db.has_password()?,
            all_browsers: true,
        })
    }
}

fn denied(msg: &str) -> Response {
    Response::Denied {
        reason: msg.to_string(),
    }
}

/// A weakening op is allowed if no password is set, or the password matches.
fn check_password(db: &Db, pw: &str) -> anyhow::Result<bool> {
    if !db.has_password()? {
        Ok(true)
    } else {
        Ok(db.verify_password(pw)?)
    }
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

/// Create the first pipe instance (so the pipe exists immediately) and spawn
/// the accept loop. Must be called from within a Tokio runtime. Returns once
/// the pipe is bound, eliminating any client startup race.
pub fn spawn_server(handler: Arc<IpcHandler>, pipe_name: String) -> std::io::Result<()> {
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&pipe_name)?;
    tokio::spawn(async move {
        if let Err(e) = serve_loop(handler, pipe_name, first).await {
            tracing::error!(error = %e, "ipc server stopped");
        }
    });
    Ok(())
}

/// Serve the named pipe forever, one instance per client, dispatching each
/// framed command through `handler`.
pub async fn serve(handler: Arc<IpcHandler>, pipe_name: String) -> anyhow::Result<()> {
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&pipe_name)?;
    serve_loop(handler, pipe_name, first).await
}

async fn serve_loop(
    handler: Arc<IpcHandler>,
    pipe_name: String,
    first: NamedPipeServer,
) -> anyhow::Result<()> {
    let mut server = first;
    loop {
        server.connect().await?;
        let connected = server;
        // Immediately stand up the next instance so no client is refused.
        server = ServerOptions::new()
            .reject_remote_clients(true)
            .create(&pipe_name)?;

        let h = handler.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_connection(connected, h).await {
                tracing::debug!(error = %e, "ipc connection ended");
            }
        });
    }
}

async fn serve_connection(mut pipe: NamedPipeServer, handler: Arc<IpcHandler>) -> anyhow::Result<()> {
    loop {
        let Some(bytes) = read_frame(&mut pipe).await? else {
            break;
        };
        let response = match proto::decode::<Command>(&bytes) {
            Ok(cmd) => handler.handle(cmd),
            Err(e) => Response::Error {
                message: format!("undecodable command: {e}"),
            },
        };
        write_frame(&mut pipe, &proto::encode(&response)).await?;
    }
    Ok(())
}

/// Client helper (also used by the UI's Rust side): send one command, read
/// one response.
pub async fn send(pipe_name: &str, cmd: &Command) -> anyhow::Result<Response> {
    let mut client = ClientOptions::new().open(pipe_name)?;
    write_frame(&mut client, &proto::encode(cmd)).await?;
    let bytes = read_frame(&mut client)
        .await?
        .ok_or_else(|| anyhow::anyhow!("service closed the pipe without responding"))?;
    Ok(proto::decode(&bytes)?)
}

async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> anyhow::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    if r.read_exact(&mut len).await.is_err() {
        return Ok(None); // peer closed
    }
    let n = u32::from_be_bytes(len) as usize;
    if n == 0 || n > MAX_FRAME {
        anyhow::bail!("invalid frame length {n}");
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

async fn write_frame<W: AsyncWriteExt + Unpin>(w: &mut W, payload: &[u8]) -> anyhow::Result<()> {
    w.write_all(&(payload.len() as u32).to_be_bytes()).await?;
    w.write_all(payload).await?;
    w.flush().await?;
    Ok(())
}

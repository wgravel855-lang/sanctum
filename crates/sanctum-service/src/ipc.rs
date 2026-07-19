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

use std::ffi::c_void;
use std::sync::OnceLock;
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

use crate::dns::Resolver;
use crate::engine;

const MAX_FRAME: usize = 1 << 20; // 1 MiB safety cap

/// Handles decoded commands against the database + running resolver.
pub struct IpcHandler {
    resolver: Arc<Resolver>,
    db_path: PathBuf,
    intervention: Arc<crate::intervention::InterventionCenter>,
}

impl IpcHandler {
    pub fn new(
        resolver: Arc<Resolver>,
        db_path: PathBuf,
        intervention: Arc<crate::intervention::InterventionCenter>,
    ) -> Self {
        Self {
            resolver,
            db_path,
            intervention,
        }
    }

    fn db(&self) -> anyhow::Result<Db> {
        Ok(Db::open(&self.db_path)?)
    }

    /// Apply the current DB state to live enforcement immediately: reload the
    /// resolver, refresh the HOSTS floor to match (an emptied list clears it),
    /// and flush cached DNS so a removed/unblocked site stops resolving to the
    /// sinkhole right away — not on the next reconcile tick, and not from a
    /// stale OS cache.
    fn apply_enforcement(&self, db: &Db) -> anyhow::Result<()> {
        // The resolver is authoritative while it's serving; the HOSTS floor is
        // owned by the reconcile loop (degraded-only). So a list/toggle change
        // just reloads the resolver and flushes cached sinkholes to take effect
        // immediately.
        engine::reload_resolver(db, &self.resolver)?;
        let _ = crate::netcfg::flush_dns_cache();
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
        // Hot-path polls that never touch the DB (the UI calls PollIntervention
        // roughly once a second).
        match &cmd {
            Command::PollIntervention => {
                let mut dto = self.intervention.poll();
                // Only touch the DB on the rare tick that actually arms a
                // window — attach the letter so the pause can show it with no
                // second round-trip. The common (nothing-pending) poll stays
                // DB-free.
                if dto.pending {
                    if let Ok(db) = self.db() {
                        dto.letter = db.get_letter().unwrap_or(None);
                    }
                }
                return Ok(Response::Intervention(dto));
            }
            Command::TriggerIntervention => {
                self.intervention.trigger_manual();
                return Ok(Response::Ok);
            }
            _ => {}
        }

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

            Command::ListCustomBlocks => Response::CustomBlocks(db.list_custom_block()?),

            // Grow-only: always allowed, even while locked.
            Command::AddBlock { domain } => {
                db.add_custom_block(&domain, now)?;
                self.apply_enforcement(&db)?;
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
                self.apply_enforcement(&db)?;
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
                self.apply_enforcement(&db)?;
                Response::Ok
            }

            Command::RemoveAllow { domain } => {
                db.remove_allow(&domain)?;
                self.apply_enforcement(&db)?;
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
                self.apply_enforcement(&db)?; // stop sinkholing + clear floor + flush
                db.record_event("protection_disabled", "", now)?;
                Response::Ok
            }

            Command::EnableProtection => {
                let mut cfg = db.load_config()?;
                cfg.protection_enabled = true;
                db.save_config(&cfg)?;
                self.apply_enforcement(&db)?; // re-arm blocking + floor + flush
                db.record_event("protection_enabled", "", now)?;
                Response::Ok
            }

            Command::SetBypassBlocking { enabled, password } => {
                // Turning it OFF is a weakening op: password-gated + frozen while
                // locked. Turning it ON always allowed (strengthens).
                if !enabled {
                    if locked {
                        return Ok(denied(
                            "Bypass blocking can't be turned off during a locked session.",
                        ));
                    }
                    if !check_password(&db, &password)? {
                        return Ok(denied("Incorrect password."));
                    }
                }
                let mut cfg = db.load_config()?;
                cfg.block_bypass = enabled;
                db.save_config(&cfg)?;
                self.apply_enforcement(&db)?;
                db.record_event(
                    if enabled { "bypass_on" } else { "bypass_off" },
                    "",
                    now,
                )?;
                Response::Ok
            }

            Command::SetStrictMode { enabled, password } => {
                // Same gating as bypass: enabling strengthens (always allowed);
                // disabling is password-gated and frozen while locked.
                if !enabled {
                    if locked {
                        return Ok(denied(
                            "Strict mode can't be turned off during a locked session.",
                        ));
                    }
                    if !check_password(&db, &password)? {
                        return Ok(denied("Incorrect password."));
                    }
                }
                let mut cfg = db.load_config()?;
                cfg.block_strict = enabled;
                db.save_config(&cfg)?;
                self.apply_enforcement(&db)?;
                db.record_event(if enabled { "strict_on" } else { "strict_off" }, "", now)?;
                Response::Ok
            }

            Command::ResolveIntervention => {
                db.record_urge_resisted(now)?;
                Response::Ok
            }

            Command::GetLetter => Response::Letter(db.get_letter()?),

            // The letter can only strengthen resolve, so it is never frozen —
            // writable even during a locked session.
            Command::SetLetter { text } => {
                db.set_letter(&text)?;
                Response::Ok
            }

            Command::DeleteHistory => {
                let count = db.delete_all_history()?;
                Response::Deleted { count }
            }

            // Handled before the DB is opened (see top of dispatch).
            Command::PollIntervention | Command::TriggerIntervention => {
                unreachable!("intervention commands are handled on the hot path")
            }
        })
    }

    fn status(&self, db: &Db, lock: &LockState, locked: bool) -> anyhow::Result<Status> {
        let cfg = db.load_config()?;
        let blocking_now =
            cfg.protection_enabled && cfg.schedule.is_active_at(chrono::Local::now());
        Ok(Status {
            protection_active: cfg.protection_enabled,
            blocking_now,
            degraded: false,
            total_blocked: db.total_blocks()?,
            urges_resisted: db.total_urges_resisted()?,
            protected_days: db.total_protected_days()?,
            streak: db.current_streak()?,
            locked,
            locked_until: lock.locked_until,
            schedule: cfg.schedule,
            blocklist_count: self.resolver.blocklist_len(),
            custom_block_count: db.list_custom_block()?.len(),
            block_bypass: cfg.block_bypass,
            block_strict: cfg.block_strict,
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

/// DACL granting Authenticated Users read+write on the pipe, with SYSTEM and
/// Administrators full control. Without an explicit descriptor, a pipe created
/// by the LocalSystem service inherits a default DACL that denies the
/// unprivileged UI — so the client's `CreateFile` fails with access-denied and
/// the app can't reach the service at all.
const PIPE_SDDL: &str = "D:(A;;GRGW;;;AU)(A;;FA;;;SY)(A;;FA;;;BA)";

/// Pointer to the pipe's security descriptor, built once from [`PIPE_SDDL`] and
/// reused for every instance. Stored as `usize` so the `OnceLock` is `Send +
/// Sync`; the descriptor is immutable after construction and lives for the
/// whole process (freed by the OS at exit), so no per-instance alloc/free is
/// needed. `0` means the descriptor couldn't be built.
static PIPE_SD: OnceLock<usize> = OnceLock::new();

fn pipe_security_descriptor() -> Option<*mut c_void> {
    let ptr = *PIPE_SD.get_or_init(|| unsafe {
        let wide: Vec<u16> = PIPE_SDDL.encode_utf16().chain(std::iter::once(0)).collect();
        let mut psd = PSECURITY_DESCRIPTOR::default();
        match ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide.as_ptr()),
            SDDL_REVISION_1,
            &mut psd,
            None,
        ) {
            Ok(()) => psd.0 as usize,
            Err(e) => {
                tracing::error!(error = %e, "failed to build pipe security descriptor");
                0
            }
        }
    });
    (ptr != 0).then_some(ptr as *mut c_void)
}

/// Create one pipe instance whose DACL admits the unprivileged UI. `first` sets
/// `first_pipe_instance` for the very first instance. Falls back to the default
/// (admin-only) DACL if the descriptor can't be built, so the service still
/// runs rather than failing to start.
fn create_pipe(pipe_name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    let mut opts = ServerOptions::new();
    opts.reject_remote_clients(true);
    if first {
        opts.first_pipe_instance(true);
    }
    match pipe_security_descriptor() {
        Some(sd) => {
            let mut sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd,
                bInheritHandle: BOOL(0),
            };
            unsafe {
                opts.create_with_security_attributes_raw(
                    pipe_name,
                    &mut sa as *mut SECURITY_ATTRIBUTES as *mut c_void,
                )
            }
        }
        None => opts.create(pipe_name),
    }
}

/// Create the first pipe instance (so the pipe exists immediately) and spawn
/// the accept loop. Must be called from within a Tokio runtime. Returns once
/// the pipe is bound, eliminating any client startup race.
pub fn spawn_server(handler: Arc<IpcHandler>, pipe_name: String) -> std::io::Result<()> {
    let first = create_pipe(&pipe_name, true)?;
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
    let first = create_pipe(&pipe_name, true)?;
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
        server = create_pipe(&pipe_name, false)?;

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

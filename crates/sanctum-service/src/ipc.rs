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
use sanctum_core::{approval, ipc as proto, password, Db};

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
                if db.load_config()?.require_partner_approval {
                    return Ok(denied(
                        "Your partner's approval is required to unblock. Request it instead.",
                    ));
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
                if db.load_config()?.require_partner_approval {
                    return Ok(denied(
                        "Your partner's approval is required to unblock. Request it instead.",
                    ));
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
                notify_partner(&db, "Protection was turned OFF.");
                Response::Ok
            }

            Command::EnableProtection => {
                let mut cfg = db.load_config()?;
                cfg.protection_enabled = true;
                db.save_config(&cfg)?;
                self.apply_enforcement(&db)?; // re-arm blocking + floor + flush
                db.record_event("protection_enabled", "", now)?;
                notify_partner(&db, "Protection was turned back on.");
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
                if !enabled {
                    notify_partner(&db, "Bypass-tool blocking (VPN/proxy/Tor) was turned off.");
                }
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
                if !enabled {
                    notify_partner(&db, "Strict mode was turned off.");
                }
                Response::Ok
            }

            Command::SetHeartbeat { enabled, password } => {
                // Enabling adds oversight (always allowed). Disabling reduces it,
                // so it is password-gated, frozen while locked, and alerts the
                // partner so the weekly signal can't be cut silently.
                if !enabled {
                    if locked {
                        return Ok(denied(
                            "The weekly check-in can't be turned off during a locked session.",
                        ));
                    }
                    if !check_password(&db, &password)? {
                        return Ok(denied("Incorrect password."));
                    }
                }
                let mut cfg = db.load_config()?;
                if cfg.heartbeat_enabled == enabled {
                    return Ok(Response::Ok); // no change
                }
                cfg.heartbeat_enabled = enabled;
                db.save_config(&cfg)?;
                db.record_event(
                    if enabled { "heartbeat_on" } else { "heartbeat_off" },
                    "",
                    now,
                )?;
                if !enabled {
                    notify_partner(&db, "Weekly protection check-ins were turned off.");
                }
                Response::Ok
            }

            Command::SetKeywordBlocking { enabled, password } => {
                // Same gating as Strict mode: enabling strengthens (always
                // allowed); disabling is password-gated and frozen while locked.
                if !enabled {
                    if locked {
                        return Ok(denied(
                            "Keyword blocking can't be turned off during a locked session.",
                        ));
                    }
                    if !check_password(&db, &password)? {
                        return Ok(denied("Incorrect password."));
                    }
                }
                let mut cfg = db.load_config()?;
                cfg.block_keywords = enabled;
                db.save_config(&cfg)?;
                self.apply_enforcement(&db)?;
                db.record_event(
                    if enabled { "keywords_on" } else { "keywords_off" },
                    "",
                    now,
                )?;
                if !enabled {
                    notify_partner(&db, "Keyword blocking was turned off.");
                }
                Response::Ok
            }

            Command::ListKeywords => Response::Keywords(db.list_custom_keyword()?),

            Command::AddKeyword { word } => {
                db.add_custom_keyword(&word, now)?;
                self.apply_enforcement(&db)?;
                db.record_event("keyword_add", word.trim(), now)?;
                Response::Ok
            }

            Command::RemoveKeyword { word, password } => {
                if locked {
                    return Ok(denied(
                        "Keyword rules can only be added during a locked session.",
                    ));
                }
                if !check_password(&db, &password)? {
                    return Ok(denied("Incorrect password."));
                }
                db.remove_custom_keyword(&word)?;
                self.apply_enforcement(&db)?;
                db.record_event("keyword_remove", word.trim(), now)?;
                Response::Ok
            }

            Command::SetPartnerApproval { enabled, password } => {
                let mut cfg = db.load_config()?;
                if cfg.require_partner_approval == enabled {
                    return Ok(Response::Ok); // no change
                }
                if enabled {
                    // Enabling adds a gate (allowed), but it would be a trap
                    // without anyone to relay codes — require a partner channel.
                    let has_channel =
                        !cfg.accountability_webhook.trim().is_empty() || cfg.sms_configured();
                    if !has_channel {
                        return Ok(denied(
                            "Set up an accountability partner first, so they can approve unblock requests.",
                        ));
                    }
                } else {
                    // Disabling removes oversight: password-gated, frozen while
                    // locked, and it alerts the partner.
                    if locked {
                        return Ok(denied(
                            "Partner approval can't be turned off during a locked session.",
                        ));
                    }
                    if !check_password(&db, &password)? {
                        return Ok(denied("Incorrect password."));
                    }
                }
                cfg.require_partner_approval = enabled;
                db.save_config(&cfg)?;
                db.clear_pending_unblock()?; // any in-flight request is moot now
                db.record_event(
                    if enabled { "partner_approval_on" } else { "partner_approval_off" },
                    "",
                    now,
                )?;
                notify_partner(
                    &db,
                    if enabled {
                        "You are now the approver for unblock requests. They'll text you a one-time code to read back when they ask to unblock a site."
                    } else {
                        "Partner approval for unblocking was turned off."
                    },
                );
                Response::Ok
            }

            Command::RequestUnblock { domain } => {
                let cfg = db.load_config()?;
                if !cfg.require_partner_approval {
                    return Ok(denied("Partner approval isn't turned on."));
                }
                if locked {
                    return Ok(denied("Unblocking is frozen during a locked session."));
                }
                let domain = domain.trim().to_lowercase();
                if domain.is_empty() {
                    return Ok(denied("Enter a site to unblock."));
                }
                // Removing the user's own block vs. allowlisting a built-in one.
                let action = if db.list_custom_block()?.iter().any(|d| d == &domain) {
                    approval::UnblockAction::RemoveBlock
                } else {
                    approval::UnblockAction::AddAllow
                };
                let code = approval::generate_code();
                let pending = approval::PendingUnblock {
                    domain: domain.clone(),
                    action,
                    code_hash: password::hash_password(&code)?,
                    created_at: now,
                    attempts: 0,
                };
                db.save_pending_unblock(&pending)?;
                db.record_event("unblock_requested", &domain, now)?;
                notify_partner(
                    &db,
                    &format!(
                        "Unblock request: they want to allow {domain}. If you approve, read them this code: {code}. If not, ignore this. (Expires in {} min.)",
                        approval::REQUEST_TTL_MINS
                    ),
                );
                Response::Ok
            }

            Command::ApproveUnblock { code } => {
                if locked {
                    return Ok(denied("Unblocking is frozen during a locked session."));
                }
                let Some(mut pending) = db.load_pending_unblock()? else {
                    return Ok(denied("There's no unblock request waiting."));
                };
                match approval::check_code(&pending, &code, now)? {
                    approval::ApprovalOutcome::Approved => {
                        match pending.action {
                            approval::UnblockAction::RemoveBlock => {
                                db.remove_custom_block(&pending.domain)?;
                            }
                            approval::UnblockAction::AddAllow => {
                                db.add_allow(&pending.domain, now)?;
                            }
                        }
                        db.clear_pending_unblock()?;
                        self.apply_enforcement(&db)?;
                        db.record_event("unblock_approved", &pending.domain, now)?;
                        notify_partner(
                            &db,
                            &format!("{} was unblocked with your approval.", pending.domain),
                        );
                        Response::Ok
                    }
                    approval::ApprovalOutcome::Wrong { attempts_left } => {
                        pending.attempts += 1;
                        db.save_pending_unblock(&pending)?;
                        if attempts_left == 0 {
                            db.clear_pending_unblock()?;
                            denied("That code was wrong too many times. Start a new request.")
                        } else {
                            denied(&format!(
                                "That code didn't match. {attempts_left} tries left."
                            ))
                        }
                    }
                    approval::ApprovalOutcome::Expired => {
                        db.clear_pending_unblock()?;
                        denied("That request expired. Start a new one.")
                    }
                    approval::ApprovalOutcome::TooManyAttempts => {
                        db.clear_pending_unblock()?;
                        denied("Too many tries. Start a new request.")
                    }
                }
            }

            Command::SetUninstallCooldown { hours } => {
                // Grow-only: enabling/increasing only strengthens (no password);
                // reducing or disabling once set is refused by the guard.
                let mut cfg = db.load_config()?;
                match config::guard_cooldown_change(cfg.uninstall_cooldown_hours, hours) {
                    Ok(accepted) => {
                        let was = cfg.uninstall_cooldown_hours;
                        cfg.uninstall_cooldown_hours = accepted;
                        // Record that this is an explicit user choice, so it
                        // survives upgrades (a stale non-zero default without this
                        // flag is healed to 0 on load — see AppConfig::migrate).
                        if accepted > 0 {
                            cfg.uninstall_cooldown_opted_in = true;
                        }
                        db.save_config(&cfg)?;
                        db.record_event(
                            "uninstall_cooldown_set",
                            &format!("{was}h -> {accepted}h"),
                            now,
                        )?;
                        Response::Ok
                    }
                    Err(e) => denied(&e.to_string()),
                }
            }

            Command::SetAccountability { webhook, password } => {
                let mut cfg = db.load_config()?;
                let old = cfg.accountability_webhook.trim().to_string();
                let new = webhook.trim().to_string();
                if old == new {
                    return Ok(Response::Ok); // no change
                }
                // Changing or removing an EXISTING partner is a weakening op:
                // password-gated, frozen while locked, and it alerts the current
                // partner first — oversight can't be cut silently.
                if !old.is_empty() {
                    if locked {
                        return Ok(denied(
                            "The accountability partner is frozen during a locked session.",
                        ));
                    }
                    if !check_password(&db, &password)? {
                        return Ok(denied("Incorrect password."));
                    }
                    crate::notifier::notify(
                        &old,
                        &stamped(if new.is_empty() {
                            "The accountability partner was removed from Sanctum."
                        } else {
                            "The accountability channel was changed to a different one."
                        }),
                    );
                }
                cfg.accountability_webhook = new.clone();
                db.save_config(&cfg)?;
                db.record_event(
                    if new.is_empty() { "accountability_off" } else { "accountability_set" },
                    "",
                    now,
                )?;
                if !new.is_empty() {
                    crate::notifier::notify(
                        &new,
                        &stamped(
                            "You are now an accountability partner for a Sanctum user. You'll get \
                             a short note if their protection is turned off or weakened. Sanctum \
                             never shares what they browse.",
                        ),
                    );
                }
                Response::Ok
            }

            Command::SetAccountabilitySms { sid, token, from, to, password } => {
                let mut cfg = db.load_config()?;
                let had = cfg.sms_configured();
                let removing = sid.trim().is_empty()
                    || token.trim().is_empty()
                    || from.trim().is_empty()
                    || to.trim().is_empty();
                // Changing or removing an existing SMS channel is weakening.
                if had {
                    if locked {
                        return Ok(denied(
                            "The accountability partner is frozen during a locked session.",
                        ));
                    }
                    if !check_password(&db, &password)? {
                        return Ok(denied("Incorrect password."));
                    }
                    notify_partner(
                        &db,
                        if removing {
                            "SMS accountability was removed."
                        } else {
                            "The SMS accountability details were changed."
                        },
                    );
                }
                cfg.sms_account_sid = sid.trim().to_string();
                cfg.sms_auth_token = token.trim().to_string();
                cfg.sms_from = from.trim().to_string();
                cfg.sms_to = to.trim().to_string();
                db.save_config(&cfg)?;
                db.record_event(
                    if cfg.sms_configured() { "accountability_sms_set" } else { "accountability_sms_off" },
                    "",
                    now,
                )?;
                if cfg.sms_configured() && !had {
                    crate::notifier::send_sms(
                        &cfg.sms_account_sid,
                        &cfg.sms_auth_token,
                        &cfg.sms_from,
                        &cfg.sms_to,
                        "Sanctum: you're now an accountability partner. You'll get a text if protection is turned off or weakened.",
                    );
                }
                Response::Ok
            }

            Command::TestAccountability => {
                let cfg = db.load_config()?;
                let w = cfg.accountability_webhook.trim();
                let mut any = false;
                if !w.is_empty() {
                    crate::notifier::notify(
                        w,
                        &stamped("Test from Sanctum. Your accountability channel is working."),
                    );
                    any = true;
                }
                if cfg.sms_configured() {
                    crate::notifier::send_sms(
                        &cfg.sms_account_sid,
                        &cfg.sms_auth_token,
                        &cfg.sms_from,
                        &cfg.sms_to,
                        "Sanctum: test message. Your SMS accountability is working.",
                    );
                    any = true;
                }
                if any {
                    Response::Ok
                } else {
                    denied("No accountability partner is set.")
                }
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
        let accountability_on = !cfg.accountability_webhook.trim().is_empty();
        let accountability_sms_on = cfg.sms_configured();
        let accountability_ntfy_topic = cfg
            .accountability_webhook
            .trim()
            .strip_prefix("https://ntfy.sh/")
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        let heartbeat_on = cfg.heartbeat_enabled;
        let require_partner_approval = cfg.require_partner_approval;
        let pending_unblock = db.load_pending_unblock()?.and_then(|p| {
            if p.is_expired(chrono::Utc::now()) {
                None
            } else {
                Some(p.domain)
            }
        });
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
            block_keywords: cfg.block_keywords,
            custom_keyword_count: db.list_custom_keyword()?.len(),
            uninstall_cooldown_hours: cfg.uninstall_cooldown_hours,
            accountability_on,
            accountability_sms_on,
            accountability_ntfy_topic,
            heartbeat_on,
            require_partner_approval,
            pending_unblock,
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

/// Prefix an accountability message with a local timestamp.
fn stamped(text: &str) -> String {
    format!(
        "Sanctum · {}\n{text}",
        chrono::Local::now().format("%b %d, %I:%M %p")
    )
}

/// Post a short accountability signal to the user's webhook if one is set.
/// Best-effort and non-blocking (the notifier spawns its own thread). Only ever
/// sends the given short text — never any browsing content.
fn notify_partner(db: &Db, text: &str) {
    if let Ok(cfg) = db.load_config() {
        let w = cfg.accountability_webhook.trim();
        if !w.is_empty() {
            crate::notifier::notify(w, &stamped(text));
        }
        if cfg.sms_configured() {
            crate::notifier::send_sms(
                &cfg.sms_account_sid,
                &cfg.sms_auth_token,
                &cfg.sms_from,
                &cfg.sms_to,
                &format!("Sanctum: {text}"),
            );
        }
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

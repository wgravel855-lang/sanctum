# ADR-001: Sanctum Enforcement Core

- **Status:** Proposed baseline (pending owner sign-off on scope — see "Scope decisions for the owner" at the end)
- **Date:** 2026-07-17
- **Deciders:** Synthesized from four independent candidate designs (reliability, windows-correctness, honesty-antifootgun, simplicity), judged and reconciled.
- **Context repo:** `C:\Users\wgrav\dev\sanctum` — workspace: `crates/sanctum-core` (lib), `crates/sanctum-service` (bin), `crates/sanctum-watchdog` (bin), `ui/` (Tauri). MSVC toolchain, Rust stable, `panic = "abort"`.

## 1. Governing principles (the two rules that break every tie)

1. **Friction, not imprisonment.** Sanctum binds the *consenting owner's* future behavior. A standard-user session is hard-blocked; an admin (the same person, mid-relapse) meets deliberate, bounded speed bumps. We never try to "win" against the machine's own admin — winning against your own admin *is* the bricking risk. There are always exactly two guaranteed, by-design exits: **the timer** (releases with no secret) and **Windows Safe Mode** (nothing of ours runs there).
2. **Never permanently break the user's internet.** `adapter DNS = 127.0.0.1/::1` makes our resolver a single point of total-internet failure. Every failure path is designed so short failures stay fail-closed (still filtered), and only a *confirmed sustained* failure degrades — and even then it degrades to a HOSTS floor, never to fully open, and never to a DNS black hole.

The **HOSTS layer is the always-on floor**: a small curated top-N of the worst domains mapped to `0.0.0.0`/`::`, needing *no running process*. It keeps blocking through resolver death, the boot window, and SAFE_FALLBACK. The full large blocklist lives *only* in the DNS resolver.

## 2. Component / crate layout

Keep the lean topology. New capabilities are **modules** in `sanctum-core`; the "extra tier" is a Scheduled Task, not a resident service. One new binary (`sanctum-recover`).

**Binaries:**
- `sanctum-service` (LocalSystem, auto-start) — SCM host via `windows-service`. Owns the DNS resolver, reconcile loop, IPC pipe server, and the SQLite DB. **Sole writer of all system state.**
- `sanctum-watchdog` (LocalSystem, auto-start) — thin second service. Mutual supervision + heartbeat/canary + circuit breaker.
- `sanctum-recover` (**new**) — standalone, service-independent. (a) `\Sanctum\BootReconcile` Scheduled-Task action (SYSTEM, at-boot + every 5 min); (b) hand-runnable Safe-Mode teardown tool shipped in the install dir.
- `sanctum-ui` (interactive user, **no admin**) — Tauri tray app, IPC client only.

**`sanctum-core` modules** — existing: `domain`, `blocklist`, `hosts`, `safesearch`, `password`, `storage`, `config`, `paths`, `error`. To add: `dns`, `netcfg`, `firewall`, `ipc`, `lock`, `timekeeper`, `acl`, `regbackup`, `supervise`, `reconcile`.

New workspace deps: `windows`, `windows-service`, `hickory-server`, `hickory-resolver`, `hickory-proto`, `idna`, `eventlog`, `tokio-util`.

## 3. DNS resolver design (`sanctum-core::dns`)

**3.1 Listeners — dual-stack, mandatory.** UDP and TCP on **both** `127.0.0.1:53` and `[::1]:53`. TCP is not optional (truncated/EDNS retries). **Do NOT set `SO_REUSEADDR`** (enables socket hijacking on Windows). Never bind `0.0.0.0`/`::`.

**3.2 Bind-and-self-verify BEFORE repointing (anti-brick gate).**
1. Open DB; hydrate desired state.
2. Bind the four sockets. On `WSAEADDRINUSE`/10048: do NOT touch adapter DNS. Find owner PID via `GetExtendedUdpTable`/`GetExtendedTcpTable`, log honestly, run **HOSTS-only** mode.
3. Self-query loopback for a canary; require the fixed answer before proceeding.
4. Serve **forward-only first**; load the full blocklist async off the SCM start thread (HOSTS floor covers the gap).
5. Only then save each adapter's current DNS (guard against capturing `127.0.0.1` as "previous") and repoint (§6).

**3.3 Per-query pipeline (normalized lowercase FQDN, priority order).**
1. Health canary `health.sanctum.invalid` → fixed answer.
2. DoH-disable canary `use-application-dns.net` → **NXDOMAIN**.
3. Allowlist → forward.
4. Blocklist (suffix match) → synthesize `A 0.0.0.0` / `AAAA ::`, TTL 0–5s. **Both families always** (no IPv6 leak).
5. DoH endpoint sink → `0.0.0.0`/`::` (data-driven, grow-only list in DB).
6. SafeSearch — see §3.4.
7. Else forward to upstream pool (§3.5), honor TTL, bounded LRU cache.

**3.4 SafeSearch — CNAME chaining.** A bare CNAME reads as incomplete to the Windows stub resolver and SafeSearch silently fails. Synthesize the CNAME **plus** the target's upstream-resolved `A`/`AAAA`. Never hardcode VIP IPs. Cover all google ccTLDs; YouTube strict; Bing strict; DDG safe.

**3.5 Upstream forwarding & fallback.** `hickory-resolver` from **captured previous per-adapter upstreams**, ordered pool `[DhcpNameServer, gateway, 1.1.1.1, 9.9.9.9]`, fast failover, ~2s timeout. **Hard loop guard: never forward to `127.0.0.1`/`::1`/self.** Re-derive network DNS from registry `DhcpNameServer` on `NotifyIpInterfaceChange` (keeps laptops working across Wi-Fi/captive portals). Static-`1.1.1.1`-only is rejected (breaks captive portals + corp names).

**3.6 Port-53-in-use.** Detect owner, surface honestly, stay HOSTS-only, retry bind on the reconcile tick. Never repoint without a verified listener.

## 4. Supervision / watchdog model + service recovery

Four layers; the **reconcile loop is the heart** (restart-each-other alone misses silent drift).

- **Tier 0 — Task Scheduler `\Sanctum\BootReconcile`** (`sanctum-recover`, SYSTEM; at-boot + every 5 min + on-event). The only layer that survives an out-of-band `sc delete` of *both* services: re-asserts SCM failure actions, re-asserts auto-start, self-heals orphaned loopback DNS.
- **Tier 1 — SCM failure actions** on both services: `reset=86400`, `restart/1000/5000/30000`, `FAILURE_ACTIONS_FLAG=1` (restart on clean non-zero exit too).
- **Tier 2 — declarative reconcile loop** every ~15–20s and on change events: re-assert sinkhole health, adapter DNS==loopback, hosts hash, firewall rules, browser DoH policy, watchdog RUNNING. Stands down when `enforcement_enabled` is false (authorized unlock/uninstall) so the user can actually leave.
- **Tier 3 — mutual health canary** (catches hangs): 3 consecutive canary failures ⇒ `TerminateProcess` ⇒ SCM restart. Asymmetric/idempotent recovery (only SCM/Task Scheduler start processes), `Global\Sanctum` singleton mutex, circuit breaker + token-bucket (≤~5/min), WER dialogs suppressed.

**Liveness state machine:** `HEALTHY` → `DEGRADED` (running but canary fails) → `DOWN` (not running) → `CRASH_LOOP` (≥5 failed recoveries/3 min ⇒ SAFE_FALLBACK).

## 5. Fail-safety rules

1. **Internal resolver errors** → keep process alive, **degrade to forward-everything** in the live resolver. Don't touch adapters. HOSTS floor still blocks.
2. **Process crash/hang** → SCM + watchdog restart in ~1–5s. Adapters stay at loopback: **fail-closed**, brief outage, still filtered.
3. **Confirmed crash-loop only** → **SAFE_FALLBACK**: restore adapter DNS to saved upstream (machine usable) but **keep the HOSTS floor**, tear down egress-53 filters, loud Event Log entry, UI = "degraded / HOSTS-only", retry every 60s. Default ON, configurable (strict-mode users may disable and accept brick-risk).

**Rejected:** pure fail-open (fully removes blocking) and fail-closed-forever (bricks on bad update).

**Stop/kill semantics:** restore adapter DNS **only on authorized (unlocked) stop**. On crash/force-kill/`sc delete`, leave adapters at loopback (fail-closed); Tiers 0/2 heal orphans. Handle `PRESHUTDOWN` to persist state; do NOT restore DNS on reboot (filtering survives reboot). DB corruption never unlocks (see §7 regbackup).

## 6. Adapter DNS set/restore mechanism

**Primary: `SetInterfaceDnsSettings`/`GetInterfaceDnsSettings` (`iphlpapi`, `netioapi.h`, Win10 2004+), keyed by interface GUID/LUID** — not localized netsh names. `netsh` is fallback only.

- Enumerate `GetAdaptersAddresses(AF_UNSPEC)`; keep `IfOperStatusUp`, exclude loopback, **keep VPN/tunnel/tether**.
- v4: `Flags=DNS_SETTING_NAMESERVER`, `NameServer="127.0.0.1"`. v6: `+DNS_SETTING_IPV6`, `NameServer="::1"` (**the IPv6-leak fix**). Don't disable IPv6 system-wide.
- **Loopback only — no real secondary** (Windows fails over to a working secondary and sticks ~15 min = one-line bypass).
- **Persist prior per-adapter config incl. DHCP-vs-static origin** before overwriting (DB journal + `regbackup`). Restore DHCP→`source=dhcp`, static→saved list.
- **React to change**: subscribe `NotifyIpInterfaceChange` + `NotifyUnicastIpAddressChange`; re-apply on adapter arrival + 15–20s reconcile. `DnsFlushResolverCache` after every change.

## 7. Storage + ACL model

`C:\ProgramData\Sanctum\` — `sanctum.db` (rusqlite bundled, WAL, `synchronous=FULL`, `busy_timeout`), `dns-restore.json`, `hosts.bak`, `logs/`, `heartbeat`. **Service is the only process that opens the DB;** UI is file-blind (IPC only).

**DACL — PROTECTED (non-inherited) via `SetNamedSecurityInfo`, re-asserted every service start:** Owner=SYSTEM; SYSTEM=Full; `Administrators`=Full (honest default — admin can take ownership regardless); `Users`=no access.

**Schema:** `settings`, `blocklist`, `allowlist`, `doh_endpoints`, `safesearch_targets`, `lock(active, mode, unlock_at, duration_ms, started_wall, monotonic_elapsed_ms, max_observed_wall)`, `password(phc)`, `intentional_stop`, `dns_restore`, `audit_log` (append-only, user-inspectable). No SQLCipher in v0.1.

**`regbackup`:** duplicate DNS-restore data + lock expiry into ACL-locked `HKLM\SOFTWARE\Sanctum` so DB corruption can never lose restore data or accidentally unlock/forever-lock.

## 8. Lock / Cold-Turkey enforcement invariants

Service is sole authority; every mutating IPC command validated server-side. Invariants in `sanctum-core::lock` as pure functions:
- **Grow-only blocklist:** add allowed while locked; remove refused.
- **Extend-only timer:** `SetUnlockAt(t)` accepted only if `t ≥ current` **and** `t ≤ now + MAX_LOCK_DURATION`. **Clamp at EVERY write** (no bug/fat-finger/bad-clock can write a forever-lock). `MAX_LOCK_DURATION = 90 days`.
- **Frozen while locked:** password change, schedule weakening, disable/pause, uninstall — refused with honest copy naming the Safe-Mode exit.
- **Release** when accrued real elapsed ≥ duration (automatic, no secret) **or** correct argon2id password (shortcut only).
- **Refuse STOP two ways:** (a) drop `SERVICE_ACCEPT_STOP` from the control mask while locked (services.msc Stop greys out); (b) mutual supervision + `intentional_stop` sentinel for the force-kill path.

**Clock-tamper defense (`timekeeper`):** accumulate monotonic elapsed (`QueryUnbiasedInterruptTime`/`GetTickCount64` + persisted carry across reboots); track `max_observed_wall`, refuse to credit large forward wall-clock jumps. Combine with MAX clamp + Safe Mode.

**Escape hatches (ethical core):** both services `SERVICE_AUTO_START` only, never under `HKLM\...\Control\SafeBoot`. Uninstaller stays in Programs & Features, detects `SM_CLEANBOOT != 0` → unconditionally bypasses the lock; MSI custom action **allows** uninstall when it can't reach the service pipe.

## 9. Hosts-section management rules

Manage only `# >>> SANCTUM START` … `# <<< SANCTUM END`.
- Resolve hosts path from registry `DataBasePath` (not hardcoded).
- Atomic write: same-dir temp → `ReplaceFileW` → `DnsFlushResolverCache`. Clear read-only first; retry on sharing violations.
- ASCII/UTF-8 **no BOM**, preserve CRLF.
- **Keep the section small** (Defender `HostsFileHijack` + dnscache perf cliff).
- **Keep the default hosts ACL** — no deny-write-for-admin ACEs (breaks legit installers, crosses the imprisonment line). Re-apply via a `ReadDirectoryChangesW` tamper-watcher.

## 10. DoH / egress hardening

Three layers:
1. **Browser DoH policy** (cheap, honest, reconcile-re-asserted): Chrome/Edge `HKLM\...\DnsOverHttpsMode="off"`, Firefox `policies.json` `DNSOverHTTPS Locked`. Required because Chrome ignores the NXDOMAIN canary.
2. **NXDOMAIN canary + DoH-hostname sink** in the resolver.
3. **Firewall/WFP egress lockdown** (visible "Sanctum" group): DoH-IP:443 blocks as **persistent** Windows Firewall rules (can't brick DNS); outbound 53/853-except-service as **WFP `FWPM_SESSION_FLAG_DYNAMIC`** filters that self-delete on crash (egress-53 + dead service is a worse brick than loopback).

**Disclosed residual v0.1 gaps:** DoH-by-hardcoded-IP not on the list; full-tunnel VPN with in-tunnel resolver (no kernel driver in v0.1).

## 11. Signing & shippability

Authenticode-sign all binaries — an unsigned LocalSystem service touching DNS/firewall/hosts will be flagged/quarantined by Defender. Keep the hosts section small.

---

## Decisions (contested points, resolved)

1. **Fail-open vs fail-closed:** Layered — degrade-to-forward in live process; fail-closed SCM restart on crash; SAFE_FALLBACK (restore upstream, keep HOSTS floor) only on confirmed crash-loop. Reject pure fail-open and fail-closed-forever.
2. **Clock-tamper:** Build monotonic accrual + max_observed_wall + MAX clamp. Not pure uptime accrual.
3. **Egress/DoH scope:** Include browser policy + canary + DoH-hostname sink (mandatory/cheap); DoH-IP:443 persistent firewall rules; 53/853-except-service via dynamic WFP session.
4. **Supervision topology:** reconcile loop (heart) + SCM failure actions + watchdog service + Task Scheduler reconciler.
5. **Storage DACL:** PROTECTED non-inherited; Users no access; Admins Full; re-assert each start.
6. **Refuse-STOP:** both control-mask drop AND supervision/sentinel.
7. **Hosts ACL:** default ACL + marker-confined atomic writes + tamper-watcher; no deny ACEs.
8. **Adapter DNS:** `SetInterfaceDnsSettings` GUID-keyed, v4+v6 separate, loopback-only, persist DHCP-vs-static.
9. **Crate topology:** lean workspace + modules + one new `sanctum-recover` binary. No 9-crate/3-service sprawl.
10. **DB-corruption vs lock:** duplicate restore data + lock expiry into ACL-locked HKLM; on DB failure enter a safe state that does NOT extend the lock.

## Windows gotcha checklist

See `phase1Tasks` and §3–§11. Key items: bind+canary before repoint; no `SO_REUSEADDR`; dual-stack v4+v6; `SetInterfaceDnsSettings` GUID-keyed; SafeSearch CNAME chaining; loopback-only (no secondary); persist DHCP origin; loop-guard the forwarder; react to adapter changes; registry `DataBasePath`; atomic `ReplaceFileW` no-BOM CRLF; `SERVICE_START_PENDING` checkpoints + off-thread startup; SCM failure actions + flag=1; auto-start-only (Safe Mode escape); drop `SERVICE_ACCEPT_STOP` while locked; restart-storm guards; PROTECTED DACL; hardened pipe; monotonic clock defense + MAX clamp; dynamic-session WFP egress; browser DoH policy; Authenticode-sign; disclose residual gaps.

## Phase-1 task order

1. Add deps + register new modules + `sanctum-recover` bin.
2. `lock` + `timekeeper` (pure, exhaustively tested — the ethical core, correct first).
3. `storage` + `acl` + `regbackup` (schema, PROTECTED DACL, HKLM backup).
4. `netcfg` (adapter enum, `SetInterfaceDnsSettings` v4+v6, change subscription, netsh fallback).
5. `hosts` (registry path, atomic writer, tamper-watcher, seed HOSTS floor).
6. `dns` (hickory sockets, per-query pipeline, upstream pool + loop-guard, bind+verify canary).
7. `firewall` (browser DoH policy, persistent DoH-IP rules, dynamic-session egress filters).
8. `ipc` (hardened pipe, typed commands, argon2id-gated + server-validated).
9. `sanctum-service` (dispatcher, startup ordering, control-mask toggle, PRESHUTDOWN, teardown gating).
10. `reconcile` + `supervise` (loop, liveness state machine, breaker/token-bucket/sentinel/mutex).
11. `sanctum-watchdog` (canary + service-status supervision + idempotent recovery).
12. `sanctum-recover` + installer (Task Scheduler action, Safe-Mode teardown, WiX, signing).
13. End-to-end VM verification of all acceptance criteria.

## Scope decisions for the owner (pending sign-off)

The following go **beyond the original v0.1 spec's enforcement model** and are held for owner approval before implementation (spec: "ask me before making architectural changes to the enforcement model"):
- **Firewall/WFP egress lockdown** (§10) — blocks plaintext DNS to non-Sanctum resolvers + DoH IPs. Strongest anti-bypass, but touches the system firewall and raises brick/Defender-flag risk (mitigated by the dynamic WFP session).
- **Browser enterprise DoH policies** (§10) — writes Chrome/Edge registry + Firefox `policies.json`. Cheap and effective; modifies browser-managed settings.
- **Clock-tamper monotonic defense** (§8) — cheap; closes the signature clock-forward bypass.
- **Task Scheduler recovery tier + `sanctum-recover` binary** (§4 Tier 0) — anti-brick net that survives `sc delete`; adds a component beyond service+watchdog.

Non-negotiable anti-brick safety (dual-stack DNS, bind-before-repoint, HOSTS floor, SAFE_FALLBACK, ACL-locked storage, Safe-Mode escape, lock invariants + MAX clamp) is treated as part of implementing the spec's stated model responsibly, not as an optional add-on.

# Sanctum

A free, open-source adult-content blocker for Windows — built as an honest
**Ulysses contract** you make with yourself. You install it on your own machine,
today, to bind your own behavior tomorrow. It is meant to outlast a craving, not
to be impossible to remove.

Sanctum is in the same category as Cold Turkey and Covenant Eyes. It is a
digital-wellbeing tool, not spyware and not a parental-surveillance product.

---

## What it does — and what it deliberately does not

**It does:**

- Block a curated (and extensible) list of adult domains in **every browser**,
  including Incognito/Private windows, with **no browser extension** — because
  filtering happens at the DNS and hosts layers below the browser.
- Force **SafeSearch** on Google, Bing, and DuckDuckGo, and **Restricted Mode**
  on YouTube.
- Keep working when the app window is closed, and across reboots, via a
  LocalSystem Windows service plus a companion watchdog.
- Offer a **locked ("Cold Turkey") session** whose settings are frozen, whose
  block list can only grow, and whose timer can only be extended.
- Track a private, **on-device-only** streak and block count, with a working
  "Delete all history" button.

**It deliberately does not:**

- Install a kernel driver, hide its processes, block Task Manager, or touch
  other software. Every component is visible in `services.msc` and Task Manager.
  This is **friction, not stealth**.
- Claim to be unremovable. There are always two guaranteed exits: **the timer**
  (releases with no secret) and **Windows Safe Mode** (nothing of Sanctum's runs
  there — see [Removing Sanctum](#removing-sanctum)).
- Send any telemetry, analytics, or personal data anywhere. There are no
  accounts and no backend. See [Privacy](#privacy).

If you are looking for a tool to monitor or restrict *someone else*, Sanctum is
not it. It only makes sense as a promise to yourself.

---

## How it works

Four visible components, each doing one job:

| Component | Runs as | Job |
|---|---|---|
| `sanctum-service` | LocalSystem service | The DNS sinkhole resolver, hosts-section enforcement, adapter DNS management, the reconcile loop, and the IPC pipe server. The sole writer of all system state. |
| `sanctum-watchdog` | LocalSystem service | Restarts `sanctum-service` if it stops; runs an end-to-end DNS liveness canary. |
| `sanctum-recover` | Standalone tool | Restores your network and removes every Sanctum change — the guaranteed manual escape hatch (see below). |
| Sanctum (UI) | Normal user, **no admin** | A Tauri desktop app. It never touches admin APIs; it reaches the service only through a local named pipe. |

### Filtering layers

1. **HOSTS floor** — a small curated set of the worst domains written into the
   `# >>> SANCTUM START` … `# <<< SANCTUM END` block of the Windows hosts file.
   It needs no running process, so it keeps blocking even during the boot window
   or a service restart. Sanctum only ever edits between its own markers.
2. **DNS sinkhole** — a resolver on `127.0.0.1:53` and `[::1]:53` (UDP + TCP).
   Active adapters' DNS (IPv4 **and** IPv6) is pointed at it. Blocked names
   resolve to `0.0.0.0` / `::`; everything else is forwarded to your previous
   upstream. This is what covers every browser and Incognito.
3. **SafeSearch / Restricted Mode** — the resolver answers the search hosts with
   a CNAME to their "safe" variant (`forcesafesearch.google.com`,
   `strict.bing.com`, `restrict.youtube.com`, `safe.duckduckgo.com`) **and
   chains in** the resolved address, because a bare CNAME silently fails on the
   Windows stub resolver.
4. **DoH containment** — the resolver sinkholes the hostnames of well-known
   DNS-over-HTTPS providers and returns NXDOMAIN for Firefox's auto-DoH probe, so
   browsers fall back to plaintext DNS that Sanctum can filter. Windows Firewall
   also blocks the known DoH resolver IPs on :443.

### Anti-brick design

Pointing your adapter DNS at a local resolver makes that resolver a single point
of total-internet failure, so every failure path is designed to never leave you
with a dead network:

- The resolver **binds and self-verifies before** any adapter is repointed. If
  port 53 is already taken, Sanctum stays HOSTS-only and never hijacks DNS with
  no listener behind it.
- A crash is covered by fast SCM restart + the watchdog (brief outage, still
  filtered). A *sustained* crash-loop degrades to the HOSTS floor with normal
  DNS restored — it never fully unblocks, and never black-holes DNS.
- Prior per-adapter DNS is journaled before any change, so it can be restored
  exactly.

---

## Build from source

Requirements: **Rust** (stable, MSVC toolchain), **Node 18+**, and the WebView2
runtime (present on Windows 10/11 by default).

```powershell
# 1. Rust workspace: core lib, service, watchdog, recover tool.
cargo build --release

# 2. UI dependencies + a production web build (or run the dev server).
cd ui
npm install
npm run build          # or: npm run tauri dev   (full desktop app)
```

The release binaries land in `target\release\`:
`sanctum-service.exe`, `sanctum-watchdog.exe`, `sanctum-recover.exe`.

### Integrity check

Validates that every blocklist parses, has no duplicate domains, and that the
live hosts file's Sanctum markers are balanced:

```powershell
py tools\integrity_check.py
```

---

## Install and run (elevated)

> Installing a LocalSystem service and changing adapter DNS requires
> Administrator rights. Sanctum's UI never has them — only the CLI installer
> does, at your explicit request.

```powershell
# Put all three binaries in one directory, then, from an elevated prompt:
.\sanctum-service.exe install       # registers + starts the service and watchdog
# ... use the app ...
.\sanctum-service.exe uninstall     # refused while a lock is active (see below)
```

For development you can run the service in the foreground instead of installing
it: `.\sanctum-service.exe console` (elevated, Ctrl+C to stop).

---

## Verifying it works (the ritual)

Run these on a real machine after installing. See `tools\verify.ps1` for the
automated parts.

1. **Blocked in every browser + Incognito, no extension.** With protection on,
   open a blocked domain in Chrome, Edge, Firefox, and an Incognito/Private
   window. Each should fail to resolve.
2. **SafeSearch / Restricted Mode.** `google.com/search` enforces SafeSearch;
   `youtube.com` is in Restricted Mode.
3. **Survives closing the UI and killing the service.** Close the app — filtering
   continues. Kill `sanctum-service.exe` in Task Manager — the watchdog restarts
   it within seconds.
4. **Locked session.** Start a lock. Confirm: `uninstall` refuses, the timer
   can't be shortened, the block list can't shrink, settings need the password
   and stay frozen, and the "Stop" button for the service is greyed out in
   `services.msc`.
5. **Honest copy.** The lock screen states the Safe-Mode escape hatch plainly.
6. **Delete all history.** The button wipes the local activity log immediately
   (verify the log is empty and the database file shrinks).
7. **No outbound traffic** except the optional blocklist-update fetch (not built
   in v0.1 — there is currently no outbound HTTP at all).
8. **Zero telemetry** — see below.

---

## Privacy

Sanctum has **no accounts, no backend, no analytics, and no telemetry.** All
state — your block list, streak, and activity log — lives only in
`C:\ProgramData\Sanctum\sanctum.db` on your machine, ACL-locked so a standard
user can't hand-edit it during a locked session. Passwords are stored only as an
Argon2id hash, never in plaintext.

You can prove the absence of telemetry yourself:

```powershell
# No analytics/telemetry SDKs anywhere:
findstr /S /I /M "analytics telemetry sentry mixpanel segment amplitude posthog gtag" crates\*.rs ui\src\*.ts ui\src\*.tsx
# No HTTP-client crate is a dependency (the only network I/O is DNS forwarding):
findstr /S /I /M "reqwest hyper ureq isahc" Cargo.toml
# No fetch/XHR to any external host in the UI:
findstr /S /I /M "fetch( XMLHttpRequest axios sendBeacon" ui\src\*.ts ui\src\*.tsx
```

All three return nothing.

---

## Cold Turkey mode — honest framing

When you start a locked session:

> **Locked until {date/time}.** This can't be turned off early from inside the
> app. Removing it before then requires booting Windows into Safe Mode — that
> friction is the point. It's meant to outlast a craving, not to be impossible.

During a lock: settings, block list, schedule, and password are frozen; the block
list may be **added to** but never shrunk; the timer may be **extended** but
never cut; the uninstaller and "disable protection" refuse. The lock duration is
clamped to a 90-day maximum at every write, so no bug or clock change can ever
create a permanent trap.

### Removing Sanctum

- **When unlocked:** run `sanctum-service.exe uninstall`, or uninstall from the
  app. Adapter DNS is restored and the hosts section is removed.
- **While locked:** wait for the timer, **or** reboot into Windows Safe Mode
  (nothing of Sanctum's runs there) and run `sanctum-recover.exe` — it restores
  your DNS and removes every change, then prints the manual `netsh` commands as a
  final fallback.

Sanctum is honest friction, not a prison. An administrator can always remove it;
the friction is deliberate and bounded.

---

## Honest limitations (v0.1)

- A determined user with admin rights can remove Sanctum via Safe Mode. That is
  by design.
- **DoH by hardcoded IP:** an app configured to reach a DNS-over-HTTPS server by
  raw IP (not on our list) can bypass the DNS layer. The optional WFP
  port-53/853 egress lockdown addresses the plaintext variant but ships **off by
  default** until verified on your hardware (a wrong filter could disrupt the
  service's own forwarding); enable it in settings once you've tested it.
- **Full-tunnel VPNs** with their own in-tunnel resolver can carry DNS past
  Sanctum. A kernel-level fix is out of scope for v0.1.
- Keyword blocking in v0.1 means the SafeSearch enforcement above — Sanctum does
  **not** scan page content.

---

## Sustainability

Sanctum is free and open-source under the **GPLv3**, and protection will **never
be paywalled** — copyleft is a deliberate choice for a recovery tool. If it helps
you and you want to support its upkeep, optional donations (GitHub Sponsors /
Ko-fi) are welcome, but every feature that protects you will always be free.

---

## Project layout

```
crates/sanctum-core/      Shared logic: domain matching, hosts section, storage,
                          Argon2 passwords, lock invariants, IPC protocol types.
crates/sanctum-service/   LocalSystem service: DNS resolver, netcfg, hosts writer,
                          firewall, IPC server, the enforcement engine.
crates/sanctum-watchdog/  The companion supervisor service.
crates/sanctum-recover/   The Safe-Mode / manual teardown tool.
ui/                       Tauri 2 + React + TypeScript + Tailwind v4 app.
blocklist/                Curated adult-domain, DoH-endpoint, and SafeSearch lists.
docs/ADR-001-*.md         The enforcement-core architecture decision record.
tools/                    Integrity check, verification script, icon generator.
```

## License

GPLv3. See [LICENSE](LICENSE).

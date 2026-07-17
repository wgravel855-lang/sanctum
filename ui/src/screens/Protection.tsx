import { useState } from "react";
import type { Response, Status } from "../lib/types";
import { dateTimeHuman, untilHuman } from "../lib/format";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Switch from "../components/Switch";
import SecuritySection from "../components/SecuritySection";

const DURATIONS = [
  { label: "1 hour", minutes: 60 },
  { label: "Tonight · 8h", minutes: 480 },
  { label: "1 day", minutes: 1440 },
  { label: "1 week", minutes: 10080 },
];

export default function Protection({
  status,
  onBack,
  refresh,
}: {
  status: Status | null;
  onBack: () => void;
  refresh: () => void;
}) {
  const locked = !!status?.locked;
  const active = !!status?.protection_active;
  const degraded = !!status?.degraded;
  const hasPassword = !!status?.has_password;

  const [busy, setBusy] = useState(false);
  const [armCT, setArmCT] = useState(false); // Cold Turkey armed, choosing duration
  const [pwPrompt, setPwPrompt] = useState(false); // disabling needs a password
  const [password, setPassword] = useState("");
  const [note, setNote] = useState<string | null>(null);

  const handle = async (r: Response, okMsg?: string) => {
    if (r.resp === "ok") setNote(okMsg ?? null);
    else if (r.resp === "denied") setNote(r.body.reason);
    else if (r.resp === "error") setNote(r.body.message);
    await refresh();
  };

  const toggleProtection = async (next: boolean) => {
    setNote(null);
    if (next) {
      setBusy(true);
      await handle(await sendCommand({ cmd: "enable_protection" }), "Protection on.");
      setBusy(false);
    } else if (hasPassword) {
      setPwPrompt(true); // ask for the password before disabling
    } else {
      setBusy(true);
      await handle(await sendCommand({ cmd: "disable_protection", password: "" }), "Protection off.");
      setBusy(false);
    }
  };

  const confirmDisable = async () => {
    setBusy(true);
    await handle(await sendCommand({ cmd: "disable_protection", password }), "Protection off.");
    setPassword("");
    setPwPrompt(false);
    setBusy(false);
  };

  const startLock = async (minutes: number) => {
    setBusy(true);
    setNote(null);
    // A lock forces protection on for its duration.
    if (!active) await sendCommand({ cmd: "enable_protection" });
    await handle(await sendCommand({ cmd: "start_lock", minutes }));
    setArmCT(false);
    setBusy(false);
  };

  const protectionSubtitle = locked
    ? "Locked on"
    : degraded
      ? "Degraded — HOSTS-only"
      : active
        ? "Active"
        : "Off";

  return (
    <div className="animate-rise">
      <TopBar title="Protection" onBack={onBack} />

      {/* Protection on/off */}
      <div className="rounded-2xl border border-border bg-surface p-5">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-[15px] font-medium text-text">Protection</div>
            <div className="mt-0.5 text-xs text-muted">{protectionSubtitle}</div>
          </div>
          <Switch
            checked={active || locked}
            disabled={locked || busy}
            onChange={toggleProtection}
            label="Protection"
          />
        </div>

        {pwPrompt && (
          <div className="mt-4 border-t border-border pt-4">
            <p className="mb-2 text-xs text-muted">Enter your password to turn protection off.</p>
            <div className="flex gap-2">
              <input
                type="password"
                autoFocus
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && confirmDisable()}
                placeholder="Password"
                className="flex-1 rounded-lg border border-border bg-surface-2 px-3 py-2 text-sm outline-none focus:border-accent"
              />
              <button
                onClick={confirmDisable}
                disabled={busy}
                className="rounded-lg bg-surface-2 px-3 py-2 text-sm text-danger"
              >
                Turn off
              </button>
              <button
                onClick={() => {
                  setPwPrompt(false);
                  setPassword("");
                }}
                className="rounded-lg px-2 py-2 text-sm text-muted"
              >
                Cancel
              </button>
            </div>
          </div>
        )}

        <div className="mt-4 flex items-center justify-between border-t border-border pt-4 text-xs">
          <span className="text-muted">Coverage</span>
          <span className="font-medium text-text">All browsers · DNS + hosts</span>
        </div>
      </div>

      {/* Cold Turkey mode */}
      <div className="mt-4 rounded-2xl border border-border bg-surface p-5">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-[15px] font-medium text-text">Cold Turkey mode</div>
            <div className="mt-0.5 text-xs text-muted">
              {locked ? `Locked · ${untilHuman(status?.locked_until ?? null)} left` : "Off"}
            </div>
          </div>
          <Switch
            checked={locked || armCT}
            disabled={locked || busy}
            onChange={(next) => {
              setNote(null);
              setArmCT(next);
            }}
            label="Cold Turkey mode"
          />
        </div>

        {locked ? (
          <p className="mt-4 border-t border-border pt-4 text-xs leading-relaxed text-text/80">
            Locked until {dateTimeHuman(status?.locked_until ?? null)}. This can't be turned off
            early from inside the app — removing it before then requires booting Windows into Safe
            Mode. That friction is the point. It's meant to outlast a craving, not to be impossible.
          </p>
        ) : armCT ? (
          <div className="mt-4 border-t border-border pt-4">
            <p className="mb-3 text-xs leading-relaxed text-muted">
              Choose how long to lock. While locked, settings freeze, the block list can only grow,
              and the timer can only be extended. <span className="text-text">You won't be able to
              turn this off early.</span>
            </p>
            <div className="grid grid-cols-2 gap-2">
              {DURATIONS.map((d) => (
                <button
                  key={d.minutes}
                  disabled={busy}
                  onClick={() => startLock(d.minutes)}
                  className="rounded-xl border border-border bg-surface-2 py-2.5 text-sm transition-colors hover:border-accent/40 disabled:opacity-60"
                >
                  Lock {d.label}
                </button>
              ))}
            </div>
          </div>
        ) : null}
      </div>

      {note && <p className="mt-3 text-center text-xs text-muted">{note}</p>}

      <SecuritySection status={status} refresh={refresh} />
    </div>
  );
}

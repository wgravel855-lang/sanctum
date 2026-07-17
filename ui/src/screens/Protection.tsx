import { useState } from "react";
import type { Status } from "../lib/types";
import { dateTimeHuman, untilHuman } from "../lib/format";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import SecuritySection from "../components/SecuritySection";

const DURATIONS = [
  { label: "1 hour", minutes: 60 },
  { label: "Tonight (8h)", minutes: 480 },
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
  const [busy, setBusy] = useState(false);
  const locked = !!status?.locked;

  const startLock = async (minutes: number) => {
    setBusy(true);
    await sendCommand({ cmd: "start_lock", minutes });
    await refresh();
    setBusy(false);
  };

  return (
    <div className="animate-rise">
      <TopBar title="Protection" onBack={onBack} />

      <div className="rounded-2xl border border-border bg-surface p-5">
        <div className="flex items-center justify-between">
          <span className="text-sm text-muted">Status</span>
          <span className="text-sm font-medium text-text">
            {status?.degraded ? "Degraded" : status?.protection_active ? "Active" : "Off"}
          </span>
        </div>
        <div className="mt-3 flex items-center justify-between border-t border-border pt-3">
          <span className="text-sm text-muted">Coverage</span>
          <span className="text-sm font-medium text-text">All browsers · DNS + hosts</span>
        </div>
      </div>

      {locked ? (
        <div className="mt-6 rounded-2xl border border-accent/30 bg-accent-soft p-5">
          <div className="text-sm font-medium text-accent">
            Locked · {untilHuman(status?.locked_until ?? null)} left
          </div>
          <p className="mt-2 text-xs leading-relaxed text-text/80">
            Locked until {dateTimeHuman(status?.locked_until ?? null)}. This can't be
            turned off early from inside the app. Removing it before then requires
            booting Windows into Safe Mode — that friction is the point. It's meant to
            outlast a craving, not to be impossible.
          </p>
        </div>
      ) : (
        <>
          <h2 className="mt-8 mb-3 text-sm font-medium text-muted">
            Start a locked session
          </h2>
          <div className="grid grid-cols-2 gap-3">
            {DURATIONS.map((d) => (
              <button
                key={d.minutes}
                disabled={busy}
                onClick={() => startLock(d.minutes)}
                className="rounded-xl border border-border bg-surface py-3 text-sm transition-colors hover:border-accent/40 disabled:opacity-50"
              >
                {d.label}
              </button>
            ))}
          </div>
          <p className="mt-4 text-xs leading-relaxed text-muted">
            While locked, settings freeze, the block list can only grow, and the timer
            can only be extended — never shortened. The only ways out are waiting for the
            timer or booting into Safe Mode. Sanctum is honest friction, not a prison.
          </p>
        </>
      )}

      <SecuritySection status={status} refresh={refresh} />
    </div>
  );
}

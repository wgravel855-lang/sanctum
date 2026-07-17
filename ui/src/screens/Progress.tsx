import { useEffect, useState } from "react";
import type { Status, EventDto } from "../lib/types";
import { commas } from "../lib/format";
import { recentEvents, sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";

function Stat({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-col items-center rounded-2xl border border-border bg-surface py-5">
      <span className="text-2xl font-semibold tabular-nums text-text">{value}</span>
      <span className="mt-1 text-xs text-muted">{label}</span>
    </div>
  );
}

export default function Progress({
  status,
  onBack,
}: {
  status: Status | null;
  onBack: () => void;
  refresh: () => void;
}) {
  const [events, setEvents] = useState<EventDto[]>([]);
  const [confirming, setConfirming] = useState(false);

  const load = () => recentEvents(50).then(setEvents);
  useEffect(() => {
    load();
  }, []);

  const wipe = async () => {
    await sendCommand({ cmd: "delete_history" });
    setConfirming(false);
    load();
  };

  return (
    <div className="animate-rise">
      <TopBar title="Progress" onBack={onBack} />

      <div className="grid grid-cols-3 gap-3">
        <Stat value={`${status?.streak ?? 0}`} label="Day streak" />
        <Stat value={`${status?.protected_days ?? 0}`} label="Days protected" />
        <Stat value={status ? commas(status.total_blocked) : "0"} label="Blocked" />
      </div>

      <p className="mt-6 text-xs text-muted">
        Everything here is stored only on this device. Nothing is uploaded, ever.
      </p>

      <h2 className="mt-8 mb-3 text-sm font-medium text-muted">Recent activity</h2>
      <div className="space-y-2">
        {events.length === 0 && (
          <p className="rounded-xl border border-border bg-surface px-4 py-3 text-sm text-muted">
            No activity recorded.
          </p>
        )}
        {events.map((e, i) => (
          <div
            key={i}
            className="flex items-center justify-between rounded-xl border border-border bg-surface px-4 py-2.5"
          >
            <span className="text-sm text-text">{e.detail}</span>
            <span className="text-xs text-muted">
              {new Date(e.ts).toLocaleDateString()}
            </span>
          </div>
        ))}
      </div>

      <div className="mt-8">
        {!confirming ? (
          <button
            onClick={() => setConfirming(true)}
            className="w-full rounded-xl border border-border bg-surface py-3 text-sm text-danger transition-colors hover:border-danger/40"
          >
            Delete all history
          </button>
        ) : (
          <div className="rounded-xl border border-danger/30 bg-surface p-4 text-center">
            <p className="text-sm text-text">
              Permanently wipe the local activity log? This can't be undone.
            </p>
            <div className="mt-3 flex gap-2">
              <button
                onClick={() => setConfirming(false)}
                className="flex-1 rounded-lg border border-border py-2 text-sm"
              >
                Cancel
              </button>
              <button
                onClick={wipe}
                className="flex-1 rounded-lg bg-danger py-2 text-sm text-white"
              >
                Delete
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

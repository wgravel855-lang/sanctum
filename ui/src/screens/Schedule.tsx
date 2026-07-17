import { useEffect, useState } from "react";
import type { Schedule as ScheduleType, Status } from "../lib/types";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";

type Mode = "always_on" | "windows" | "focus" | "off";

const MODES: { key: Mode; label: string; desc: string }[] = [
  { key: "always_on", label: "Always on", desc: "Enforced 24/7." },
  { key: "windows", label: "Nightly window", desc: "e.g. 9:00 PM–6:00 AM, when you're most vulnerable." },
  { key: "focus", label: "Focus session", desc: "A one-off block for a set stretch of time." },
  { key: "off", label: "Off", desc: "No schedule (protection can still be on manually)." },
];

const FOCUS_OPTIONS = [
  { label: "30 min", mins: 30 },
  { label: "1 hour", mins: 60 },
  { label: "2 hours", mins: 120 },
  { label: "4 hours", mins: 240 },
];

const minToTime = (m: number) =>
  `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
const timeToMin = (t: string) => {
  const [h, m] = t.split(":").map(Number);
  return h * 60 + m;
};

export default function Schedule({
  status,
  onBack,
  refresh,
}: {
  status: Status | null;
  onBack: () => void;
  refresh: () => void;
}) {
  const locked = !!status?.locked;
  const hasPassword = !!status?.has_password;

  const [mode, setMode] = useState<Mode>("always_on");
  const [start, setStart] = useState("21:00");
  const [end, setEnd] = useState("06:00");
  const [focusMins, setFocusMins] = useState(60);
  const [password, setPassword] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!status) return;
    const s = status.schedule;
    setMode(s.mode);
    if (s.mode === "windows" && s.windows[0]) {
      setStart(minToTime(s.windows[0].start_min));
      setEnd(minToTime(s.windows[0].end_min));
    }
  }, [status]);

  const build = (): ScheduleType => {
    switch (mode) {
      case "always_on":
        return { mode: "always_on" };
      case "off":
        return { mode: "off" };
      case "windows":
        return { mode: "windows", windows: [{ start_min: timeToMin(start), end_min: timeToMin(end), days: [] }] };
      case "focus":
        return { mode: "focus", ends_at: new Date(Date.now() + focusMins * 60_000).toISOString() };
    }
  };

  const save = async () => {
    setSaving(true);
    setNote(null);
    const r = await sendCommand({ cmd: "set_schedule", schedule: build(), password });
    if (r.resp === "ok") {
      setNote("Schedule saved.");
      await refresh();
    } else if (r.resp === "denied") {
      setNote(r.body.reason);
    } else {
      setNote("Couldn't save the schedule.");
    }
    setPassword("");
    setSaving(false);
  };

  return (
    <div className="animate-rise">
      <TopBar title="Schedule" onBack={onBack} />

      {locked && (
        <p className="mb-5 rounded-xl border border-accent/30 bg-accent-soft px-4 py-3 text-sm text-accent">
          The schedule is frozen while a locked session is active.
        </p>
      )}

      <div className="space-y-3">
        {MODES.map((m) => {
          const selected = m.key === mode;
          return (
            <button
              key={m.key}
              disabled={locked}
              onClick={() => setMode(m.key)}
              className={`w-full rounded-2xl border p-4 text-left transition-colors disabled:opacity-60 ${
                selected ? "border-accent bg-accent-soft" : "border-border bg-surface hover:border-accent/40"
              }`}
            >
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium text-text">{m.label}</span>
                {selected && <span className="text-xs font-medium text-accent">Selected</span>}
              </div>
              <p className="mt-1 text-xs text-muted">{m.desc}</p>
            </button>
          );
        })}
      </div>

      {mode === "windows" && !locked && (
        <div className="mt-4 rounded-2xl border border-border bg-surface p-4">
          <div className="flex items-center gap-3">
            <label className="flex-1 text-sm text-muted">
              From
              <input
                type="time"
                value={start}
                onChange={(e) => setStart(e.target.value)}
                className="mt-1 w-full rounded-lg border border-border bg-surface-2 px-3 py-2 text-text outline-none focus:border-accent"
              />
            </label>
            <label className="flex-1 text-sm text-muted">
              Until
              <input
                type="time"
                value={end}
                onChange={(e) => setEnd(e.target.value)}
                className="mt-1 w-full rounded-lg border border-border bg-surface-2 px-3 py-2 text-text outline-none focus:border-accent"
              />
            </label>
          </div>
          <p className="mt-3 text-xs text-muted">
            Overnight windows (like 9:00 PM–6:00 AM) are supported.
          </p>
        </div>
      )}

      {mode === "focus" && !locked && (
        <div className="mt-4 grid grid-cols-4 gap-2">
          {FOCUS_OPTIONS.map((o) => (
            <button
              key={o.mins}
              onClick={() => setFocusMins(o.mins)}
              className={`rounded-xl border py-2.5 text-sm transition-colors ${
                focusMins === o.mins ? "border-accent bg-accent-soft text-accent" : "border-border bg-surface"
              }`}
            >
              {o.label}
            </button>
          ))}
        </div>
      )}

      {!locked && (
        <div className="mt-6">
          {hasPassword && (
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Settings password"
              className="mb-3 w-full rounded-xl border border-border bg-surface px-4 py-2.5 text-sm outline-none focus:border-accent"
            />
          )}
          <button
            onClick={save}
            disabled={saving}
            className="w-full rounded-xl bg-accent py-3 text-sm font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-60"
          >
            {saving ? "Saving…" : "Save schedule"}
          </button>
          {note && <p className="mt-2 text-center text-xs text-muted">{note}</p>}
        </div>
      )}
    </div>
  );
}

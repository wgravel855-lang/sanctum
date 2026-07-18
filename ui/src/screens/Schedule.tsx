import { useEffect, useState } from "react";
import type { Schedule as ScheduleType, Status } from "../lib/types";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Button from "../components/Button";
import { Group, GroupFootnote, GroupLabel, Row } from "../components/List";
import { CheckIcon } from "../components/icons";

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
    setNote(r.resp === "ok" ? "Schedule saved." : r.resp === "denied" ? r.body.reason : "Couldn't save the schedule.");
    if (r.resp === "ok") await refresh();
    setPassword("");
    setSaving(false);
  };

  return (
    <div className="screen">
      <TopBar title="Schedule" onBack={onBack} />

      <Group>
        {MODES.map((m) => {
          const selected = m.key === mode;
          return (
            <Row key={m.key} onClick={locked ? undefined : () => setMode(m.key)} disabled={locked}>
              <span className="flex flex-col">
                <span className="t-row-title">{m.label}</span>
                <span className="t-caption">{m.desc}</span>
              </span>
              {selected && (
                <span className="row-trailing text-accent">
                  <CheckIcon className="h-5 w-5" />
                </span>
              )}
            </Row>
          );
        })}
      </Group>

      {mode === "windows" && !locked && (
        <div className="mt-8">
          <GroupLabel>Window</GroupLabel>
          <div className="flex gap-3">
            <label className="flex-1 t-caption">
              From
              <input type="time" value={start} onChange={(e) => setStart(e.target.value)} className="field mt-1" />
            </label>
            <label className="flex-1 t-caption">
              Until
              <input type="time" value={end} onChange={(e) => setEnd(e.target.value)} className="field mt-1" />
            </label>
          </div>
          <GroupFootnote>Overnight windows (like 9:00 PM–6:00 AM) are supported.</GroupFootnote>
        </div>
      )}

      {mode === "focus" && !locked && (
        <div className="mt-8">
          <GroupLabel>Duration</GroupLabel>
          <div className="grid grid-cols-4 gap-2">
            {FOCUS_OPTIONS.map((o) => (
              <button
                key={o.mins}
                onClick={() => setFocusMins(o.mins)}
                className={`pressable rounded-[10px] border py-2.5 text-[15px] ${
                  focusMins === o.mins ? "border-accent text-accent" : "border-hairline text-text-1"
                }`}
              >
                {o.label}
              </button>
            ))}
          </div>
        </div>
      )}

      {locked ? (
        <GroupFootnote>The schedule is frozen while a locked session is active.</GroupFootnote>
      ) : (
        <div className="mt-8">
          {hasPassword && (
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Settings password"
              className="field mb-3"
            />
          )}
          <Button onClick={save} disabled={saving}>
            {saving ? "Saving…" : "Save schedule"}
          </Button>
          {note && <p className="t-caption mt-2 text-center">{note}</p>}
        </div>
      )}
    </div>
  );
}

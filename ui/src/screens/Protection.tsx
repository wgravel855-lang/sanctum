import { useState } from "react";
import type { Response, Status } from "../lib/types";
import { dateTimeHuman, untilHuman } from "../lib/format";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Switch from "../components/Switch";
import Button from "../components/Button";
import SecuritySection from "../components/SecuritySection";
import { Group, GroupFootnote, Row } from "../components/List";

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
  const [armCT, setArmCT] = useState(false);
  const [pwPrompt, setPwPrompt] = useState(false);
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
      setPwPrompt(true);
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
    if (!active) await sendCommand({ cmd: "enable_protection" });
    await handle(await sendCommand({ cmd: "start_lock", minutes }));
    setArmCT(false);
    setBusy(false);
  };

  const protectionSubtitle = locked ? "Locked on" : degraded ? "Degraded, HOSTS-only" : active ? "Active" : "Off";

  return (
    <div className="screen">
      <TopBar title="Protection" onBack={onBack} />

      <Group>
        <Row>
          <span className="flex flex-col">
            <span className="t-row-title">Protection</span>
            <span className="t-caption">{protectionSubtitle}</span>
          </span>
          <span className="row-trailing">
            <Switch checked={active || locked} disabled={locked || busy} onChange={toggleProtection} label="Protection" />
          </span>
        </Row>
        <Row>
          <span className="t-row-title">Coverage</span>
          <span className="row-trailing t-subtitle">All browsers · DNS + hosts</span>
        </Row>
      </Group>

      {pwPrompt && (
        <div className="mt-3">
          <p className="t-caption mb-2 px-4">Enter your password to turn protection off.</p>
          <input
            type="password"
            autoFocus
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && confirmDisable()}
            placeholder="Password"
            className="field"
          />
          <div className="mt-3 flex items-center justify-between">
            <button
              onClick={() => {
                setPwPrompt(false);
                setPassword("");
              }}
              className="pressable text-[15px] text-text-2"
            >
              Cancel
            </button>
            <Button variant="destructive" onClick={confirmDisable}>
              Turn protection off
            </Button>
          </div>
        </div>
      )}

      {/* Cold Turkey */}
      <div className="mt-8">
        <Group>
          <Row>
            <span className="flex flex-col">
              <span className="t-row-title">Cold Turkey mode</span>
              <span className="t-caption">
                {locked ? `Locked · ${untilHuman(status?.locked_until ?? null)} left` : "Off"}
              </span>
            </span>
            <span className="row-trailing">
              <Switch
                checked={locked || armCT}
                disabled={locked || busy}
                onChange={(next) => {
                  setNote(null);
                  setArmCT(next);
                }}
                label="Cold Turkey mode"
              />
            </span>
          </Row>
        </Group>

        {armCT && !locked && (
          <div className="mt-3">
            <p className="t-caption mb-3 px-1">
              Choose how long to lock. While locked, settings freeze and the block list can only
              grow. <span className="text-text-1">You won't be able to turn this off early.</span>
            </p>
            <div className="grid grid-cols-2 gap-2">
              {DURATIONS.map((d) => (
                <button
                  key={d.minutes}
                  disabled={busy}
                  onClick={() => startLock(d.minutes)}
                  className="pressable rounded-[10px] border border-hairline py-3 text-[15px] text-text-1 disabled:opacity-50"
                >
                  Lock {d.label}
                </button>
              ))}
            </div>
          </div>
        )}

        {locked && (
          <GroupFootnote>
            Locked until {dateTimeHuman(status?.locked_until ?? null)}. This can't be turned off
            early from inside the app. Removing it before then requires booting Windows into Safe
            Mode. That friction is the point. It's meant to outlast a craving, not to be impossible.
          </GroupFootnote>
        )}
      </div>

      {note && <p className="t-caption mt-4 text-center">{note}</p>}

      <SecuritySection status={status} refresh={refresh} />
    </div>
  );
}

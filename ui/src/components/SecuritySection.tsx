import { useState } from "react";
import type { Status } from "../lib/types";
import { sendCommand } from "../lib/ipc";
import Button from "./Button";
import { GroupFootnote, GroupLabel } from "./List";

/** Set or change the settings password. The button stays a visibly-inert
 *  Secondary until both fields validate, then becomes Primary. */
export default function SecuritySection({
  status,
  refresh,
}: {
  status: Status | null;
  refresh: () => void;
}) {
  const hasPassword = !!status?.has_password;
  const locked = !!status?.locked;

  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  if (locked) {
    return (
      <p className="mt-8 border-t border-hairline pt-6 t-caption">
        The settings password is frozen while a locked session is active.
      </p>
    );
  }

  const valid = next.length >= 4 && next === confirm && (!hasPassword || current.length > 0);

  const submit = async () => {
    if (!valid) return;
    setBusy(true);
    setNote(null);
    const r = await sendCommand({
      cmd: "set_password",
      new: next,
      current: hasPassword ? current : null,
    });
    if (r.resp === "ok") {
      setNote(hasPassword ? "Password changed." : "Password set.");
      setCurrent("");
      setNext("");
      setConfirm("");
      await refresh();
    } else if (r.resp === "denied") {
      setNote(r.body.reason);
    } else {
      setNote("Couldn't update the password.");
    }
    setBusy(false);
  };

  return (
    <div className="mt-8 border-t border-hairline pt-6">
      <GroupLabel>{hasPassword ? "Change settings password" : "Set a settings password"}</GroupLabel>
      <div className="space-y-2">
        {hasPassword && (
          <input
            type="password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
            placeholder="Current password"
            className="field"
          />
        )}
        <input
          type="password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          placeholder={hasPassword ? "New password" : "Password"}
          className="field"
        />
        <input
          type="password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          placeholder="Confirm password"
          className="field"
        />
      </div>
      <Button
        variant={valid ? "primary" : "secondary"}
        disabled={!valid || busy}
        onClick={submit}
        className="mt-3"
      >
        {busy ? "Saving…" : hasPassword ? "Change password" : "Set password"}
      </Button>
      {note && <p className="t-caption mt-2 text-center">{note}</p>}
      <GroupFootnote>
        Gates changes that weaken protection: removing sites, or turning it off. It can't unlock a
        locked session; only the timer or Safe Mode can.
      </GroupFootnote>
    </div>
  );
}

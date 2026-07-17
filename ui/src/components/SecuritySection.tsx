import { useState } from "react";
import type { Status } from "../lib/types";
import { sendCommand } from "../lib/ipc";

/** Set or change the settings password. The password gates weakening actions;
 *  it never unlocks a locked session (only the timer or Safe Mode can). */
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
      <p className="mt-8 border-t border-border pt-6 text-xs text-muted">
        The settings password is frozen while a locked session is active.
      </p>
    );
  }

  const submit = async () => {
    setNote(null);
    if (next.length < 4) {
      setNote("Choose a password of at least 4 characters.");
      return;
    }
    if (next !== confirm) {
      setNote("Passwords don't match.");
      return;
    }
    setBusy(true);
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

  const input =
    "w-full rounded-xl border border-border bg-surface px-4 py-2.5 text-sm outline-none focus:border-accent";

  return (
    <div className="mt-8 border-t border-border pt-6">
      <h2 className="mb-2 text-sm font-medium text-muted">
        {hasPassword ? "Change settings password" : "Set a settings password"}
      </h2>
      <p className="mb-3 text-xs leading-relaxed text-muted">
        Gates changes that weaken protection — removing sites, turning it off. It
        can't unlock a locked session; only the timer or Safe Mode can.
      </p>
      <div className="space-y-2">
        {hasPassword && (
          <input
            type="password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
            placeholder="Current password"
            className={input}
          />
        )}
        <input
          type="password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          placeholder={hasPassword ? "New password" : "Password"}
          className={input}
        />
        <input
          type="password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          placeholder="Confirm password"
          className={input}
        />
      </div>
      <button
        onClick={submit}
        disabled={busy}
        className="mt-3 w-full rounded-xl border border-border bg-surface py-2.5 text-sm transition-colors hover:border-accent/40 disabled:opacity-60"
      >
        {busy ? "Saving…" : hasPassword ? "Change password" : "Set password"}
      </button>
      {note && <p className="mt-2 text-center text-xs text-muted">{note}</p>}
    </div>
  );
}

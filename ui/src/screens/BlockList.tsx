import { useCallback, useEffect, useState } from "react";
import type { Status } from "../lib/types";
import { listCustomBlocks, sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Button from "../components/Button";
import { Group, GroupFootnote, GroupLabel, Row } from "../components/List";

export default function BlockList({
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

  const [custom, setCustom] = useState<string[]>([]);
  const [domain, setDomain] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pendingRemove, setPendingRemove] = useState<string | null>(null);
  const [password, setPassword] = useState("");

  const loadCustom = useCallback(async () => {
    setCustom(await listCustomBlocks());
  }, []);

  useEffect(() => {
    loadCustom();
  }, [loadCustom]);

  const add = async () => {
    const d = domain.trim().toLowerCase();
    if (!d) return;
    setBusy(true);
    const r = await sendCommand({ cmd: "add_block", domain: d });
    setNote(r.resp === "ok" ? `Added ${d}` : `Couldn't add ${d}`);
    setDomain("");
    await loadCustom();
    await refresh();
    setBusy(false);
  };

  const doRemove = async (d: string, pw: string) => {
    setBusy(true);
    setNote(null);
    const r = await sendCommand({ cmd: "remove_block", domain: d, password: pw });
    if (r.resp === "ok") setNote(`Removed ${d}`);
    else if (r.resp === "denied") setNote(r.body.reason);
    else setNote(`Couldn't remove ${d}`);
    setPendingRemove(null);
    setPassword("");
    await loadCustom();
    await refresh();
    setBusy(false);
  };

  const onRemoveClick = (d: string) => {
    setNote(null);
    if (hasPassword) setPendingRemove(d);
    else doRemove(d, "");
  };

  return (
    <div className="screen">
      <TopBar title="Block List" onBack={onBack} />

      <p className="t-body text-text-2">
        Sanctum already blocks tens of thousands of adult sites out of the box.
        Add anything else you want kept out here.
      </p>

      {/* User-added sites (removable). The built-in baseline isn't user-managed
          and isn't listed or counted here. */}
      <div className="mt-8">
        <GroupLabel>Your added sites</GroupLabel>
        {custom.length === 0 ? (
          <p className="t-caption px-4">You haven't added any sites yet.</p>
        ) : (
          <Group>
            {custom.map((d) => (
              <Row key={d}>
                <span className="t-row-title truncate">{d}</span>
                <span className="row-trailing">
                  {locked ? (
                    <span className="t-caption">Locked</span>
                  ) : (
                    <button
                      onClick={() => onRemoveClick(d)}
                      disabled={busy}
                      className="pressable text-[15px] text-destructive disabled:opacity-50"
                    >
                      Remove
                    </button>
                  )}
                </span>
              </Row>
            ))}
          </Group>
        )}

        {pendingRemove && (
          <div className="mt-3">
            <p className="t-caption mb-2 px-1">
              Enter your password to remove <span className="text-text-1">{pendingRemove}</span>.
            </p>
            <input
              type="password"
              autoFocus
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doRemove(pendingRemove, password)}
              placeholder="Password"
              className="field"
            />
            <div className="mt-3 flex items-center justify-between">
              <button
                onClick={() => {
                  setPendingRemove(null);
                  setPassword("");
                }}
                className="pressable text-[15px] text-text-2"
              >
                Cancel
              </button>
              <Button variant="destructive" onClick={() => doRemove(pendingRemove, password)}>
                Remove site
              </Button>
            </div>
          </div>
        )}
      </div>

      <div className="mt-8">
        <GroupLabel>Add a site</GroupLabel>
        <input
          value={domain}
          onChange={(e) => setDomain(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          placeholder="example.com"
          spellCheck={false}
          autoCapitalize="none"
          className="field"
        />
        <Button className="mt-3" onClick={add} disabled={busy}>
          Add site
        </Button>
        {note && <p className="t-caption mt-2 text-center">{note}</p>}
      </div>

      <GroupFootnote>
        You can always add sites. During a locked session the list can only grow, so removing a
        site is disabled until the lock ends. That friction is the point.
      </GroupFootnote>
    </div>
  );
}

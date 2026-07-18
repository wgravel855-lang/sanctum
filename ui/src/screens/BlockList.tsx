import { useState } from "react";
import type { Status } from "../lib/types";
import { commas } from "../lib/format";
import { sendCommand } from "../lib/ipc";
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
  const [domain, setDomain] = useState("");
  const [note, setNote] = useState<string | null>(null);

  const add = async () => {
    const d = domain.trim();
    if (!d) return;
    const r = await sendCommand({ cmd: "add_block", domain: d });
    setNote(r.resp === "ok" ? `Added ${d}` : `Couldn't add ${d}`);
    setDomain("");
    refresh();
  };

  return (
    <div className="screen">
      <TopBar title="Block List" onBack={onBack} />

      <Group>
        <Row>
          <span className="t-row-title">Blocked sites</span>
          <span className="row-trailing t-row-title tnum text-text-1">
            {status ? commas(status.blocklist_count) : "…"}
          </span>
        </Row>
      </Group>

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
        <Button className="mt-3" onClick={add}>
          Add site
        </Button>
        {note && <p className="t-caption mt-2 text-center">{note}</p>}
      </div>

      <GroupFootnote>
        You can always add sites. During a locked session the list can only grow, so
        removing a site is disabled until the lock ends. That friction is the point.
      </GroupFootnote>
    </div>
  );
}

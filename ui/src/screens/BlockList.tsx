import { useState } from "react";
import type { Status } from "../lib/types";
import { commas } from "../lib/format";
import { sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";

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
    <div className="animate-rise">
      <TopBar title="Block List" onBack={onBack} />

      <div className="rounded-2xl border border-border bg-surface p-5 text-center">
        <div className="text-3xl font-semibold tabular-nums text-text">
          {status ? commas(status.blocklist_count) : "—"}
        </div>
        <div className="mt-1 text-sm text-muted">sites blocked</div>
      </div>

      <h2 className="mt-8 mb-2 text-sm font-medium text-muted">Add a site</h2>
      <div className="flex gap-2">
        <input
          value={domain}
          onChange={(e) => setDomain(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
          placeholder="example.com"
          spellCheck={false}
          autoCapitalize="none"
          className="flex-1 rounded-xl border border-border bg-surface px-4 py-2.5 text-sm outline-none focus:border-accent"
        />
        <button
          onClick={add}
          className="rounded-xl bg-accent px-4 py-2.5 text-sm font-medium text-accent-contrast transition-colors hover:bg-accent-hover"
        >
          Add
        </button>
      </div>
      {note && <p className="mt-2 text-xs text-muted">{note}</p>}

      <p className="mt-6 text-xs leading-relaxed text-muted">
        You can always add sites. During a locked session the list can only
        grow — removing a site is disabled until the lock ends. That friction is
        the point.
      </p>
    </div>
  );
}

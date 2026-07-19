import { useEffect, useState } from "react";
import type { EventDto, Status } from "../lib/types";
import { aggregateActivity, commas } from "../lib/format";
import { recentEvents, sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Button from "../components/Button";
import { Group, GroupLabel, GroupFootnote, Row } from "../components/List";

// §G — the no-shame progress view. It leads with urges *resisted* (a number
// that only ever grows) instead of a streak that a single slip resets to zero.
// Nothing here frames a hard day as failure.

export default function Progress({
  status,
  onBack,
  onOpenLetter,
}: {
  status: Status | null;
  onBack: () => void;
  refresh: () => void;
  onOpenLetter: () => void;
}) {
  const [events, setEvents] = useState<EventDto[]>([]);
  const [confirming, setConfirming] = useState(false);

  const load = () => recentEvents(500).then(setEvents);
  useEffect(() => {
    load();
  }, []);

  const wipe = async () => {
    await sendCommand({ cmd: "delete_history" });
    setConfirming(false);
    load();
  };

  const activity = aggregateActivity(events);
  const resisted = status?.urges_resisted ?? 0;

  return (
    <div className="screen">
      <TopBar title="Progress" onBack={onBack} />

      {/* Urges resisted — the honest hero. It never resets. */}
      <div className="flex flex-col items-center py-4 text-center">
        <div className="t-stat text-accent tnum">{commas(resisted)}</div>
        <div className="t-subtitle mt-2">
          {resisted === 1 ? "urge resisted" : "urges resisted"}
        </div>
        <p className="t-body mt-3 max-w-xs text-text-2">
          {resisted === 0
            ? "The first time an urge shows up, this is where it gets counted. Not a slip — a rep."
            : "Every one was a moment the urge came and you stayed. That's the whole game."}
        </p>
      </div>

      {/* Secondary stats — plain, never framed as pass/fail. */}
      <div className="mt-6 grid grid-cols-3 gap-3">
        <div className="group flex flex-col items-center py-5">
          <span className="t-title tnum">{status?.streak ?? 0}</span>
          <span className="t-caption mt-1">Day streak</span>
        </div>
        <div className="group flex flex-col items-center py-5">
          <span className="t-title tnum">{status ? commas(status.protected_days) : "0"}</span>
          <span className="t-caption mt-1">Days protected</span>
        </div>
        <div className="group flex flex-col items-center py-5">
          <span className="t-title tnum">{status ? commas(status.total_blocked) : "0"}</span>
          <span className="t-caption mt-1">Blocked</span>
        </div>
      </div>
      <GroupFootnote>
        A streak is just a tally of good days, not a test. If it resets, the days
        before it still happened and still counted.
      </GroupFootnote>

      {/* Letter to self (§C). */}
      <div className="mt-8">
        <GroupLabel>When it's hard</GroupLabel>
        <Group>
          <Row onClick={onOpenLetter}>
            <span className="t-body">Letter to self</span>
            <span className="row-trailing t-caption">Shown during a block ›</span>
          </Row>
        </Group>
      </div>

      {/* Recent activity — human sentences, aggregated. */}
      <div className="mt-8">
        <GroupLabel>Recent activity</GroupLabel>
        <Group>
          {activity.length === 0 ? (
            <Row>
              <span className="t-body text-text-2">No activity recorded.</span>
            </Row>
          ) : (
            activity.slice(0, 12).map((a, i) => (
              <Row key={i}>
                <span className="t-body">{a.text}</span>
                <span className="row-trailing t-caption">{a.when}</span>
              </Row>
            ))
          )}
        </Group>
      </div>

      {/* Delete all history. */}
      <div className="mt-8 flex justify-center">
        {!confirming ? (
          <Button variant="destructive" onClick={() => setConfirming(true)}>
            Delete all history
          </Button>
        ) : (
          <div className="w-full">
            <p className="t-subtitle mb-3 text-center">
              Permanently wipe the local activity log? This can't be undone.
            </p>
            <Button variant="secondary" onClick={() => setConfirming(false)}>
              Keep history
            </Button>
            <div className="mt-3 flex justify-center">
              <Button variant="destructive" onClick={wipe}>
                Delete everything
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

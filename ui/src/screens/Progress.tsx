import { useEffect, useState } from "react";
import type { EventDto, Status } from "../lib/types";
import { aggregateActivity, commas } from "../lib/format";
import { recentEvents, sendCommand } from "../lib/ipc";
import TopBar from "../components/TopBar";
import Button from "../components/Button";
import { Group, GroupLabel, GroupFootnote, Row } from "../components/List";

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

  return (
    <div className="screen">
      <TopBar title="Progress" onBack={onBack} />

      {/* Streak — the emotional hero. */}
      <div className="flex flex-col items-center py-4">
        <div className="t-stat text-accent">{status?.streak ?? 0}</div>
        <div className="t-subtitle mt-2">day streak</div>
      </div>

      {/* Two smaller inline stats. */}
      <div className="mt-4 grid grid-cols-2 gap-3">
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
        Everything here is stored only on this device. Nothing is uploaded, ever.
      </GroupFootnote>

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

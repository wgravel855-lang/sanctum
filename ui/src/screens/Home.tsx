import type { Screen } from "../App";
import type { Status } from "../lib/types";
import { commas, untilHuman } from "../lib/format";
import { useCountUp } from "../lib/hooks";
import StatusRing from "../components/StatusRing";
import ControlButton from "../components/ControlButton";
import { ChartIcon, ClockIcon, HeartIcon, ListIcon, ShieldIcon } from "../components/icons";

export default function Home({
  status,
  onNavigate,
  onUrge,
}: {
  status: Status | null;
  onNavigate: (s: Screen) => void;
  onUrge: () => void;
}) {
  const active = !!status?.protection_active && !status?.degraded;
  const degraded = !!status?.degraded;
  const headline = degraded
    ? "Protection is degraded"
    : active
      ? "Protection is active"
      : "Protection is off";

  const blocked = useCountUp(status?.total_blocked ?? 0);

  return (
    <div className="screen flex flex-col items-center text-center">
      <StatusRing active={active} degraded={degraded} />

      <h1 className="t-hero mt-6">{headline}</h1>

      <p className="t-body mt-3 tnum text-text-2">
        {status ? `${commas(blocked)} harmful sites blocked` : " "}
      </p>
      <p className="t-caption mt-1">
        {degraded
          ? "HOSTS-only — the resolver is recovering"
          : status?.all_browsers
            ? "All browsers protected"
            : "Some browsers unprotected"}
      </p>

      {status?.locked && (
        <div className="mt-5 rounded-[100px] bg-accent-soft px-4 py-1.5 text-[13px] font-medium text-accent">
          Locked · {untilHuman(status.locked_until)} left
        </div>
      )}

      {/* 40px between the hero block and the grid */}
      <div className="mt-10 grid w-full grid-cols-2 gap-3">
        <ControlButton
          icon={<ShieldIcon />}
          label="Protection"
          sublabel={active ? "On" : degraded ? "Degraded" : "Off"}
          onClick={() => onNavigate("protection")}
        />
        <ControlButton
          icon={<ClockIcon />}
          label="Schedule"
          sublabel={scheduleLabel(status)}
          onClick={() => onNavigate("schedule")}
        />
        <ControlButton
          icon={<ListIcon />}
          label="Block List"
          sublabel={status ? `${commas(status.blocklist_count)} sites` : undefined}
          onClick={() => onNavigate("blocklist")}
        />
        <ControlButton
          icon={<ChartIcon />}
          label="Progress"
          sublabel={status ? `${status.streak}-day streak` : undefined}
          onClick={() => onNavigate("progress")}
        />
      </div>

      <button
        onClick={onUrge}
        className="pressable mt-8 flex items-center gap-2 rounded-[100px] bg-surface-2 px-5 py-2.5 text-[15px] font-medium text-text-1"
      >
        <HeartIcon className="h-4 w-4 text-accent" />
        I need help now
      </button>
    </div>
  );
}

function scheduleLabel(status: Status | null): string | undefined {
  if (!status) return undefined;
  switch (status.schedule.mode) {
    case "always_on":
      return "Always on";
    case "off":
      return "Off";
    case "windows":
      return "Scheduled";
    case "focus":
      return "Focus session";
  }
}

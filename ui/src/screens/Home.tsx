import type { Screen } from "../App";
import type { Status } from "../lib/types";
import { commas, untilHuman } from "../lib/format";
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

  return (
    <div className="flex flex-col items-center text-center animate-rise">
      <StatusRing active={active} degraded={degraded} />

      <h1 className="mt-8 font-display text-[2.1rem] leading-tight text-text">{headline}</h1>

      <p className="mt-3 text-lg text-muted">
        {status ? `${commas(status.total_blocked)} harmful sites blocked` : " "}
      </p>
      <p className="mt-1 text-sm text-muted">
        {degraded
          ? "HOSTS-only — the resolver is recovering"
          : status?.all_browsers
            ? "All browsers protected"
            : "Some browsers unprotected"}
      </p>

      {status?.locked && (
        <div className="mt-5 rounded-full bg-accent-soft px-4 py-1.5 text-sm font-medium text-accent">
          Locked · {untilHuman(status.locked_until)} left
        </div>
      )}

      <div className="mt-10 grid w-full grid-cols-2 gap-3">
        <ControlButton
          icon={<ShieldIcon />}
          label="Protection"
          sublabel={active ? "On" : "Off"}
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
        className="mt-8 flex items-center gap-2 rounded-full px-4 py-2 text-sm text-muted transition-colors hover:text-accent"
      >
        <HeartIcon className="h-4 w-4" />
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

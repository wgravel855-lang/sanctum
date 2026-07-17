import { ShieldIcon } from "./icons";

/** A calm circular status indicator. Single accent; no glow, no gradient. */
export default function StatusRing({
  active,
  degraded,
}: {
  active: boolean;
  degraded: boolean;
}) {
  const ring = degraded
    ? "border-danger/40"
    : active
      ? "border-accent"
      : "border-[var(--border)]";
  const fill = degraded
    ? "text-danger"
    : active
      ? "text-accent"
      : "text-muted";

  return (
    <div className="relative flex items-center justify-center">
      <div className={`h-32 w-32 rounded-full border-[3px] ${ring} transition-colors duration-200`} />
      <div className={`absolute inset-0 flex items-center justify-center ${fill}`}>
        <ShieldIcon className="h-14 w-14" />
      </div>
    </div>
  );
}

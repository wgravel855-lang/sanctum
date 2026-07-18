import { ShieldIcon } from "./icons";

/** A calm 88px status ring, 2px stroke. Single accent; no glow. */
export default function StatusRing({ active, degraded }: { active: boolean; degraded: boolean }) {
  const fill = degraded ? "text-destructive" : active ? "text-accent" : "text-text-3";
  const ring = degraded ? "border-destructive/40" : active ? "border-accent" : "border-hairline";
  return (
    <div className="relative flex h-[88px] w-[88px] items-center justify-center">
      <div className={`absolute inset-0 rounded-full border-2 ${ring} transition-colors duration-200`} />
      <span className={fill}>
        <ShieldIcon className="h-10 w-10" />
      </span>
    </div>
  );
}

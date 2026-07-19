import { type CSSProperties } from "react";

// The shared 4-7-8 breathing visual — a single glowing orb that expands on the
// inhale, holds, and deflates on the exhale, wrapped by a faint progress ring.
// Both the block-moment window and the in-app urge overlay render this, so the
// two feel like one calm, considered thing.

const CYCLE = [
  { label: "Breathe in", secs: 4 },
  { label: "Hold", secs: 7 },
  { label: "Breathe out", secs: 8 },
] as const;
const CYCLE_SECS = 19;

/** Derive the current phase and the seconds left in it from elapsed time, so
 *  the countdown is always live (never a static label). */
export function phaseAt(elapsed: number): { idx: number; left: number } {
  const t = elapsed % CYCLE_SECS;
  if (t < 4) return { idx: 0, left: 4 - t };
  if (t < 11) return { idx: 1, left: 11 - t };
  return { idx: 2, left: CYCLE_SECS - t };
}

const RING_R = 150;
const RING_C = 2 * Math.PI * RING_R;

export default function BreathingOrb({
  elapsed,
  totalSecs,
  reduced = false,
}: {
  elapsed: number;
  totalSecs: number;
  reduced?: boolean;
}) {
  const { idx, left } = phaseAt(elapsed);
  const cur = CYCLE[idx];
  const expanded = idx !== 2; // inhale + hold are the "full" state
  const dur = reduced ? 0 : cur.secs;
  const scale = reduced ? 0.9 : expanded ? 1 : 0.62;
  const glow = expanded ? 1 : 0.38;
  const progress = totalSecs > 0 ? Math.min(1, elapsed / totalSecs) : 0;

  const breathe: CSSProperties = {
    transition: reduced
      ? "none"
      : `transform ${dur}s cubic-bezier(0.4, 0, 0.5, 1), opacity ${dur}s ease-in-out`,
  };

  return (
    <div className="relative flex h-[340px] w-[340px] items-center justify-center">
      {/* Faint progress ring — fixed size, fills across the whole pause. No
          numeric clock: it says "this is ending" without inviting clock-watching. */}
      <svg className="absolute inset-0 -rotate-90" viewBox="0 0 340 340" aria-hidden>
        <circle cx="170" cy="170" r={RING_R} fill="none" stroke="var(--hairline)" strokeWidth="2" />
        <circle
          cx="170"
          cy="170"
          r={RING_R}
          fill="none"
          stroke="var(--accent)"
          strokeWidth="2"
          strokeLinecap="round"
          strokeOpacity="0.5"
          strokeDasharray={RING_C}
          strokeDashoffset={RING_C * (1 - progress)}
          style={{ transition: "stroke-dashoffset 1s linear" }}
        />
      </svg>

      {/* The orb: glow halo + translucent body, scaling together with the breath. */}
      <div
        className="absolute h-[236px] w-[236px]"
        style={{ transform: `scale(${scale})`, willChange: "transform", ...breathe }}
        aria-hidden
      >
        <div
          className="absolute rounded-full"
          style={{
            inset: "-44px",
            background:
              "radial-gradient(circle, color-mix(in srgb, var(--accent) 50%, transparent) 0%, transparent 68%)",
            filter: "blur(26px)",
            opacity: glow,
            ...breathe,
          }}
        />
        <div
          className="absolute inset-0 rounded-full"
          style={{
            background:
              "radial-gradient(circle at 50% 36%, color-mix(in srgb, var(--accent) 24%, transparent) 0%, color-mix(in srgb, var(--accent) 7%, transparent) 54%, transparent 72%)",
            border: "1px solid color-mix(in srgb, var(--accent) 42%, transparent)",
            boxShadow:
              "inset 0 2px 24px color-mix(in srgb, var(--accent) 16%, transparent), inset 0 -18px 32px color-mix(in srgb, black 30%, transparent)",
          }}
        />
      </div>

      {/* Phase word + live count — sits above the orb and never scales. */}
      <div className="relative z-10 flex flex-col items-center">
        <div className="text-[15px] font-medium tracking-[0.02em] text-text-2">{cur.label}</div>
        <div className="tnum mt-1 text-[46px] font-light leading-none text-text-1">{left}</div>
      </div>
    </div>
  );
}

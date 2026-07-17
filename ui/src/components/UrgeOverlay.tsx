import { useEffect, useRef, useState } from "react";

// 4-7-8 breathing: inhale 4s, hold 7s, exhale 8s (19s cycle).
const PHASES = [
  { label: "Breathe in", secs: 4, scale: 1 },
  { label: "Hold", secs: 7, scale: 1 },
  { label: "Breathe out", secs: 8, scale: 0.6 },
] as const;

const DISTRACTIONS = [
  "Stand up and get a glass of water.",
  "Step outside for two minutes of fresh air.",
  "Text someone you trust just to say hi.",
];

const SESSION_SECS = 120;

export default function UrgeOverlay({ onClose }: { onClose: () => void }) {
  const [phase, setPhase] = useState(0);
  const [remaining, setRemaining] = useState(SESSION_SECS);
  const reduced = useRef(
    typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );

  // Session countdown.
  useEffect(() => {
    const t = setInterval(() => setRemaining((r) => Math.max(0, r - 1)), 1000);
    return () => clearInterval(t);
  }, []);

  // Breathing phase cycle.
  useEffect(() => {
    const t = setTimeout(() => setPhase((p) => (p + 1) % PHASES.length), PHASES[phase].secs * 1000);
    return () => clearTimeout(t);
  }, [phase]);

  const current = PHASES[phase];
  const mm = Math.floor(remaining / 60);
  const ss = String(remaining % 60).padStart(2, "0");

  return (
    <div className="fixed inset-0 z-50 flex flex-col items-center justify-center bg-bg px-6 text-center">
      <p className="text-sm uppercase tracking-widest text-muted">This will pass</p>

      <div className="relative mt-12 flex h-56 w-56 items-center justify-center">
        <div
          className="absolute h-40 w-40 rounded-full bg-accent-soft"
          style={{
            transform: reduced.current ? "scale(1)" : `scale(${current.scale + 0.4})`,
            transition: reduced.current ? "none" : `transform ${current.secs}s ease-in-out`,
          }}
        />
        <div className="relative z-10">
          <div className="font-display text-3xl text-text">{current.label}</div>
          <div className="mt-1 text-sm text-muted">{current.secs}s</div>
        </div>
      </div>

      <div className="mt-12 text-3xl font-semibold tabular-nums text-text">
        {mm}:{ss}
      </div>
      <p className="mt-1 text-sm text-muted">Stay for two minutes.</p>

      <ul className="mt-10 w-full max-w-xs space-y-2 text-left">
        {DISTRACTIONS.map((d) => (
          <li key={d} className="rounded-xl border border-border bg-surface px-4 py-3 text-sm text-text">
            {d}
          </li>
        ))}
      </ul>

      <button
        onClick={onClose}
        className="mt-10 text-sm text-muted transition-colors hover:text-text"
      >
        {remaining === 0 ? "I'm okay now" : "Close"}
      </button>
    </div>
  );
}

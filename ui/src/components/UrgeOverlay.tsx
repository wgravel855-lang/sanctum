import { useEffect, useRef, useState } from "react";
import Button from "./Button";
import { Group, Row } from "./List";

// 4-7-8 breathing: inhale 4s, hold 7s, exhale 8s (19s cycle).
const PHASES = [
  { label: "Breathe in", secs: 4, scale: 1 },
  { label: "Hold", secs: 7, scale: 1 },
  { label: "Breathe out", secs: 8, scale: 0.55 },
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

  useEffect(() => {
    const t = setInterval(() => setRemaining((r) => Math.max(0, r - 1)), 1000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    const t = setTimeout(
      () => setPhase((p) => (p + 1) % PHASES.length),
      PHASES[phase].secs * 1000,
    );
    return () => clearTimeout(t);
  }, [phase]);

  const current = PHASES[phase];
  const mm = Math.floor(remaining / 60);
  const ss = String(remaining % 60).padStart(2, "0");

  return (
    <div className="screen fixed inset-0 z-50 flex flex-col items-center overflow-y-auto bg-bg px-5 py-12 text-center">
      <p className="t-label">This will pass</p>

      <div className="relative mt-12 flex h-56 w-56 items-center justify-center">
        <div
          className="absolute h-40 w-40 rounded-full bg-accent-soft"
          style={{
            transform: reduced.current ? "scale(1)" : `scale(${current.scale + 0.45})`,
            transition: reduced.current ? "none" : `transform ${current.secs}s ease-in-out`,
          }}
        />
        <div className="relative z-10">
          <div className="t-title">{current.label}</div>
          <div className="t-subtitle mt-1 tnum">{current.secs}s</div>
        </div>
      </div>

      <div className="mt-12 t-title tnum">
        {mm}:{ss}
      </div>
      <p className="t-caption mt-1">Stay for two minutes.</p>

      <div className="mt-10 w-full max-w-xs">
        <Group>
          {DISTRACTIONS.map((d) => (
            <Row key={d}>
              <span className="t-body">{d}</span>
            </Row>
          ))}
        </Group>
      </div>

      <div className="mt-10 w-full max-w-xs">
        <Button variant="secondary" onClick={onClose}>
          {remaining === 0 ? "I'm okay now" : "Close"}
        </Button>
      </div>
    </div>
  );
}

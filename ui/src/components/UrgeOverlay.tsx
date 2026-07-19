import { useEffect, useRef, useState } from "react";
import Button from "./Button";
import BreathingOrb from "./BreathingOrb";

// The in-app "I need help now" screen. You opened it on purpose, but it still
// holds you for a short breath before the exit appears — the pause is the whole
// point, so it can't be closed instantly.

const PAUSE_SECS = 30;

const DISTRACTIONS = [
  "Stand up and get a glass of water.",
  "Step outside for two minutes of air.",
  "Text someone you trust, just to say hi.",
];

function messageAt(elapsed: number): string {
  if (elapsed < 12) return "You came here instead of giving in. That was the strong move.";
  if (elapsed < 24) return "Let the breath be the only thing you're doing right now.";
  return "The wave is already on its way back down.";
}

export default function UrgeOverlay({ onClose }: { onClose: () => void }) {
  const [elapsed, setElapsed] = useState(0);
  const [stage, setStage] = useState<"breathe" | "ramp">("breathe");
  const reduced = useRef(
    typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );

  useEffect(() => {
    if (stage !== "breathe") return;
    const t = setInterval(() => {
      setElapsed((e) => {
        if (e + 1 >= PAUSE_SECS) {
          clearInterval(t);
          setStage("ramp");
          return PAUSE_SECS;
        }
        return e + 1;
      });
    }, 1000);
    return () => clearInterval(t);
  }, [stage]);

  return (
    <div className="fade-in fixed inset-0 z-50 flex flex-col items-center justify-center overflow-y-auto bg-bg px-8 py-12 text-center">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0"
        style={{
          background:
            "radial-gradient(circle at 50% 42%, color-mix(in srgb, var(--accent) 9%, transparent) 0%, transparent 46%), radial-gradient(circle at 50% 46%, transparent 30%, color-mix(in srgb, var(--bg) 78%, black) 100%)",
        }}
      />

      {stage === "breathe" && (
        <div className="relative flex flex-col items-center">
          <p className="t-label">This will pass</p>
          <div className="mt-8">
            <BreathingOrb elapsed={elapsed} totalSecs={PAUSE_SECS} reduced={reduced.current} />
          </div>
          <p className="t-body mt-8 max-w-md text-balance text-text-2">{messageAt(elapsed)}</p>
        </div>
      )}

      {stage === "ramp" && (
        <div className="fade-in relative flex w-full max-w-md flex-col items-center">
          <p className="t-title text-balance">You stayed with it.</p>
          <p className="t-body mt-3 max-w-sm text-balance text-text-2">
            That's the rep. If it helps, do one of these next.
          </p>

          <div className="mt-8 w-full space-y-2">
            {DISTRACTIONS.map((d) => (
              <div
                key={d}
                className="flex items-center gap-3 rounded-[12px] border border-hairline bg-surface-1/60 px-4 py-3.5 text-left t-body"
              >
                <span className="h-1.5 w-1.5 flex-none rounded-full bg-accent" />
                {d}
              </div>
            ))}
          </div>

          <div className="mt-10 w-full max-w-xs">
            <Button variant="secondary" onClick={onClose}>
              I'm okay now
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

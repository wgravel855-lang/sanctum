import { useCallback, useEffect, useRef, useState } from "react";
import Button from "../components/Button";
import { inTauri, sendCommand } from "../lib/ipc";

// v0.1.5 §B — the block-moment intervention window. Full-screen, always-on-top,
// calm. Phase 1 is a non-skippable pause; only after it do exit ramps appear.
// There is never an "unblock anyway" control.

const PAUSE_SECS = 45;

// 4-7-8 breathing.
const BREATH = [
  { label: "Breathe in", secs: 4, scale: 1.0 },
  { label: "Hold", secs: 7, scale: 1.0 },
  { label: "Breathe out", secs: 8, scale: 0.5 },
] as const;

// Static defaults for now; user-configured redirect actions land in §D.
const SUGGESTIONS = [
  "Step outside for five minutes.",
  "Drink a glass of water.",
  "Open something you're building.",
];

const RING_R = 130;
const RING_C = 2 * Math.PI * RING_R;

export default function InterventionWindow() {
  const [remaining, setRemaining] = useState(PAUSE_SECS);
  const [bi, setBi] = useState(0);
  const [stage, setStage] = useState<"breathe" | "ramp" | "closed">("breathe");
  const reduced = useRef(
    typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );

  const reset = useCallback(() => {
    setRemaining(PAUSE_SECS);
    setBi(0);
    setStage("breathe");
  }, []);

  // The window is reused across urges: reset to Phase 1 each time it's raised.
  useEffect(() => {
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen("intervention-open", () => reset()).then((u) => (un = u));
    });
    return () => un?.();
  }, [reset]);

  // The pause countdown.
  useEffect(() => {
    if (stage !== "breathe") return;
    const t = setInterval(() => {
      setRemaining((r) => {
        if (r <= 1) {
          clearInterval(t);
          setStage("ramp");
          return 0;
        }
        return r - 1;
      });
    }, 1000);
    return () => clearInterval(t);
  }, [stage]);

  // Breathing sub-phase cycle.
  useEffect(() => {
    if (stage !== "breathe") return;
    const t = setTimeout(() => setBi((p) => (p + 1) % BREATH.length), BREATH[bi].secs * 1000);
    return () => clearTimeout(t);
  }, [bi, stage]);

  // Non-dismissible during the pause: swallow Escape.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (stage === "breathe" && e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [stage]);

  const close = async () => {
    if (inTauri()) {
      await sendCommand({ cmd: "resolve_intervention" });
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().hide();
      reset();
    } else {
      setStage("closed");
    }
  };

  const cur = BREATH[bi];
  const pct = remaining / PAUSE_SECS;

  return (
    <div className="fade-in fixed inset-0 z-50 flex flex-col items-center justify-center bg-bg px-6 text-center">
      {stage === "breathe" && (
        <>
          <p className="t-label">This will pass</p>

          <div className="relative mt-10 flex h-[300px] w-[300px] items-center justify-center">
            <svg className="absolute inset-0 -rotate-90" viewBox="0 0 300 300">
              <circle cx="150" cy="150" r={RING_R} fill="none" stroke="var(--hairline)" strokeWidth="2" />
              <circle
                cx="150"
                cy="150"
                r={RING_R}
                fill="none"
                stroke="var(--accent)"
                strokeWidth="2"
                strokeLinecap="round"
                strokeDasharray={RING_C}
                strokeDashoffset={RING_C * (1 - pct)}
                style={{ transition: "stroke-dashoffset 1s linear" }}
              />
            </svg>
            <div
              className="absolute h-40 w-40 rounded-full bg-accent-soft"
              style={{
                transform: reduced.current ? "scale(1)" : `scale(${cur.scale + 0.35})`,
                transition: reduced.current ? "none" : `transform ${cur.secs}s ease-in-out`,
              }}
            />
            <div className="relative z-10">
              <div className="t-title">{cur.label}</div>
            </div>
          </div>

          <p className="t-body mt-10 max-w-sm text-text-2">
            This urge will pass. Breathe with it for a moment.
          </p>
          <p className="t-caption mt-2 tnum">{remaining}s</p>
        </>
      )}

      {stage === "ramp" && (
        <div className="fade-in w-full max-w-md">
          <p className="t-label">Your reason</p>
          <p className="t-title mt-6">You set this block while your head was clear.</p>
          <p className="t-body mt-4 text-text-2">
            It's doing exactly what you asked it to. The urge is already fading.
          </p>

          <div className="mt-10 space-y-2 text-left">
            {SUGGESTIONS.map((s) => (
              <div key={s} className="rounded-[12px] bg-surface-1 px-4 py-3 t-body">
                {s}
              </div>
            ))}
          </div>

          <div className="mt-10">
            <Button variant="secondary" onClick={close}>
              I'm okay — close
            </Button>
          </div>
        </div>
      )}

      {stage === "closed" && <p className="t-title text-text-2">Take care of yourself.</p>}
    </div>
  );
}

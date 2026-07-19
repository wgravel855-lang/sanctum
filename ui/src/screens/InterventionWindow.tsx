import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import Button from "../components/Button";
import BreathingOrb from "../components/BreathingOrb";
import { getLetter, inTauri, sendCommand } from "../lib/ipc";

// v0.1.5 §B — the block-moment window. Full-screen, always-on-top, calm.
// Phase 1 is a non-skippable 45s pause: there is no close control, and Esc /
// Alt+F4 / the X are all refused (in the Tauri shell). Only after it does the
// reflection + a single "I'm okay" appear. There is never an "unblock anyway".

const PAUSE_SECS = 45;

function messageAt(elapsed: number): string {
  if (elapsed < 15) return "Breathe with it. There's nothing else you need to do right now.";
  if (elapsed < 30) return "Most urges rise and fall within a couple of minutes. You're already inside one.";
  return "Still here. That's the whole practice — staying, not winning.";
}

const SUGGESTIONS = [
  "Step outside for five minutes.",
  "Drink a glass of water.",
  "Open something you're building.",
];

export default function InterventionWindow() {
  const [elapsed, setElapsed] = useState(0);
  const [stage, setStage] = useState<"breathe" | "ramp" | "closed">("breathe");
  const [letter, setLetter] = useState<string | null>(null);
  const [frame, setFrame] = useState<{ x: number; y: number; w: number; h: number } | null>(null);
  const reduced = useRef(
    typeof window !== "undefined" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );

  const reset = useCallback(() => {
    setElapsed(0);
    setStage("breathe");
  }, []);

  const loadLetter = useCallback(() => {
    getLetter().then(setLetter).catch(() => setLetter(null));
  }, []);

  useEffect(() => {
    loadLetter();
    if (!inTauri()) return;
    let un: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) => {
      listen("intervention-open", (e: { payload?: { frame?: typeof frame } }) => {
        setFrame(e?.payload?.frame ?? null);
        reset();
        loadLetter();
      }).then((u) => (un = u));
    });
    return () => un?.();
  }, [reset, loadLetter]);

  // One tick drives the pause; the orb derives its phase from `elapsed`.
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

  // Non-dismissible during the pause: swallow Escape (belt-and-braces with the
  // Tauri window's prevent_close).
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

  const frameStyle: CSSProperties = frame
    ? { position: "absolute", left: frame.x, top: frame.y, width: frame.w, height: frame.h }
    : { position: "absolute", inset: 0 };

  return (
    // Outer layer covers every monitor with an opaque backdrop; the inner frame
    // holds the centered content on the primary screen (others stay blank).
    <div className="fade-in fixed inset-0 z-50 overflow-hidden bg-bg">
      <div
        className="flex flex-col items-center justify-center overflow-hidden px-8 text-center"
        style={frameStyle}
      >
        {/* Warm radial glow + edge vignette pull focus to the center. */}
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
            {letter ? (
              <>
                <p className="t-label">A note you left yourself</p>
                <div className="mt-6 w-full rounded-[16px] border border-hairline bg-surface-1/70 px-6 py-6 text-left backdrop-blur-sm">
                  <p className="whitespace-pre-line text-[19px] leading-relaxed text-text-1">
                    {letter}
                  </p>
                </div>
              </>
            ) : (
              <>
                <p className="t-label">Your reason</p>
                <p className="t-title mt-6 text-balance">
                  You set this block while your head was clear.
                </p>
                <p className="t-body mt-4 max-w-sm text-balance text-text-2">
                  It's doing exactly what you asked it to. The urge is already fading.
                </p>
              </>
            )}

            <p className="t-label mt-10 self-start">A few things that help</p>
            <div className="mt-3 w-full space-y-2">
              {SUGGESTIONS.map((s) => (
                <div
                  key={s}
                  className="flex items-center gap-3 rounded-[12px] border border-hairline bg-surface-1/60 px-4 py-3.5 text-left t-body"
                >
                  <span className="h-1.5 w-1.5 flex-none rounded-full bg-accent" />
                  {s}
                </div>
              ))}
            </div>

            <div className="mt-10 w-full max-w-xs">
              <Button variant="secondary" onClick={close}>
                I'm okay — close
              </Button>
            </div>
          </div>
        )}

        {stage === "closed" && (
          <p className="relative t-title text-text-2">Take care of yourself.</p>
        )}
      </div>
    </div>
  );
}

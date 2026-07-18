import { useEffect, useRef, useState } from "react";

/** Count up to `target` once on first non-zero value (600ms, ease-out).
 *  Honours prefers-reduced-motion by jumping straight to the target. */
export function useCountUp(target: number, duration = 600): number {
  const [value, setValue] = useState(0);
  const started = useRef(false);

  useEffect(() => {
    if (started.current || target <= 0) {
      if (target <= 0) setValue(target);
      return;
    }
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      started.current = true;
      setValue(target);
      return;
    }
    started.current = true;
    const start = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / duration);
      const eased = 1 - Math.pow(1 - t, 3);
      setValue(Math.round(target * eased));
      if (t < 1) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    // Safety: if rAF never runs (some renderers), still show the final value.
    const safety = setTimeout(() => setValue(target), duration + 120);
    return () => {
      cancelAnimationFrame(raf);
      clearTimeout(safety);
    };
  }, [target, duration]);

  return value;
}

import type { ReactNode } from "react";

/** A home-screen control card. Identical geometry across all four:
 *  20px accent icon top-left, 12px gap, 17/500 title, 15 --text-2 subtitle. */
export default function ControlButton({
  icon,
  label,
  sublabel,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  sublabel?: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="pressable flex flex-col items-start gap-3 rounded-[12px] border border-hairline bg-surface-1 p-4 text-left"
    >
      <span className="text-accent [&_svg]:h-5 [&_svg]:w-5">{icon}</span>
      <span className="flex flex-col">
        <span className="t-row-title">{label}</span>
        {sublabel && <span className="t-subtitle">{sublabel}</span>}
      </span>
    </button>
  );
}

import type { ReactNode } from "react";

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
      className="group flex flex-col items-start gap-3 rounded-2xl border border-border bg-surface p-5 text-left transition-all duration-150 hover:border-accent/40 active:scale-[0.98]"
    >
      <span className="text-accent">{icon}</span>
      <span className="flex flex-col">
        <span className="text-[15px] font-medium text-text">{label}</span>
        {sublabel && <span className="text-xs text-muted">{sublabel}</span>}
      </span>
    </button>
  );
}

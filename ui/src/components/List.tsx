import type { ReactNode } from "react";

/** A section label above a group (iOS-style, 8px gap to the container). */
export function GroupLabel({ children }: { children: ReactNode }) {
  return <div className="t-label mb-2 px-4">{children}</div>;
}

/** An iOS-Settings grouped container. Rows inside are separated by hairlines. */
export function Group({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`group ${className}`}>{children}</div>;
}

/** A single row. If `onClick` is given it becomes a pressable button row. */
export function Row({
  children,
  onClick,
  disabled,
}: {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
}) {
  if (onClick) {
    return (
      <button
        type="button"
        onClick={onClick}
        disabled={disabled}
        className="row pressable w-full text-left disabled:opacity-50"
      >
        {children}
      </button>
    );
  }
  return <div className="row">{children}</div>;
}

/** A footnote below a group (iOS-style explanatory caption). */
export function GroupFootnote({ children }: { children: ReactNode }) {
  return <p className="t-caption mt-2 px-4">{children}</p>;
}

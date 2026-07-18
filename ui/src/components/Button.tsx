import type { ReactNode } from "react";

type Variant = "primary" | "secondary" | "destructive";

/** The three-and-only-three button system. */
export default function Button({
  variant = "primary",
  children,
  onClick,
  disabled = false,
  className = "",
}: {
  variant?: Variant;
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  className?: string;
}) {
  const base =
    variant === "primary"
      ? "btn btn-primary"
      : variant === "secondary"
        ? "btn btn-secondary"
        : "btn-destructive";
  return (
    <button type="button" onClick={onClick} disabled={disabled} className={`${base} ${className}`}>
      {children}
    </button>
  );
}

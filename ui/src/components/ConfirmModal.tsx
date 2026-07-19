import { useEffect } from "react";
import Button from "./Button";

// A centered confirmation dialog shown before a consequential choice (enabling
// Strict mode, arming a locked session), so the user knows exactly what they
// are agreeing to before it happens.

export default function ConfirmModal({
  open,
  title,
  body,
  confirmLabel = "Continue",
  cancelLabel = "Cancel",
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  body: string;
  confirmLabel?: string;
  cancelLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  // Escape cancels; the backdrop is not a commit path.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  if (!open) return null;

  return (
    <div
      className="fade-in fixed inset-0 z-[60] flex items-center justify-center px-6"
      style={{ background: "color-mix(in srgb, var(--bg) 55%, transparent)", backdropFilter: "blur(6px)" }}
      onClick={onCancel}
    >
      <div
        role="dialog"
        aria-modal="true"
        className="w-full max-w-sm rounded-[16px] border border-hairline bg-surface-1 p-6 text-left shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="t-title">{title}</h2>
        <p className="t-body mt-3 whitespace-pre-line text-text-2">{body}</p>
        <div className="mt-7 flex flex-col gap-2">
          <Button variant="primary" onClick={onConfirm}>
            {confirmLabel}
          </Button>
          <Button variant="secondary" onClick={onCancel}>
            {cancelLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}

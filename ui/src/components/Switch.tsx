/** An accessible iOS-style toggle. Accent when on; respects prefers-reduced-
 *  motion (transitions are disabled globally under that media query). */
export default function Switch({
  checked,
  onChange,
  disabled = false,
  label,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => !disabled && onChange(!checked)}
      className={`relative h-[28px] w-[46px] shrink-0 rounded-full outline-none transition-colors duration-200 focus-visible:ring-2 focus-visible:ring-accent/60 disabled:cursor-not-allowed disabled:opacity-45 ${
        checked ? "bg-accent" : "border border-border bg-surface-2"
      }`}
    >
      <span
        className={`absolute left-[3px] top-[3px] h-[22px] w-[22px] rounded-full bg-white shadow-sm transition-transform duration-200 ${
          checked ? "translate-x-[18px]" : "translate-x-0"
        }`}
      />
    </button>
  );
}

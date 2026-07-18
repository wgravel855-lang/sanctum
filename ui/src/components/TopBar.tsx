import { BackIcon } from "./icons";

export default function TopBar({ title, onBack }: { title: string; onBack: () => void }) {
  return (
    <div className="mb-8 flex items-center gap-3">
      <button
        onClick={onBack}
        aria-label="Back"
        className="-ml-2 flex h-11 w-11 items-center justify-center rounded-full text-text-1 pressable"
      >
        <BackIcon />
      </button>
      <h1 className="t-title">{title}</h1>
    </div>
  );
}

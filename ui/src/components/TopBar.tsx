import { BackIcon } from "./icons";

export default function TopBar({ title, onBack }: { title: string; onBack: () => void }) {
  return (
    <div className="mb-8 flex items-center gap-2">
      <button
        onClick={onBack}
        aria-label="Back"
        className="-ml-2 flex h-11 w-11 items-center justify-center rounded-full text-muted transition-colors hover:bg-surface-2 hover:text-text"
      >
        <BackIcon />
      </button>
      <h1 className="font-display text-2xl text-text">{title}</h1>
    </div>
  );
}

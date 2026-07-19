import { useEffect, useRef, useState } from "react";
import TopBar from "../components/TopBar";
import Button from "../components/Button";
import { GroupFootnote } from "../components/List";
import { getLetter, setLetter as saveLetter } from "../lib/ipc";

// §C — the letter to self. Written in a calm moment, shown during the
// block-moment pause. It is never frozen by a lock (it can only help), so it
// can be edited any time.

const PLACEHOLDER =
  "Write to the version of you who will be having a hard moment.\n\nWhy did you set this up? What do you actually want? What would you tell a friend feeling this?";

export default function Letter({ onBack }: { onBack: () => void }) {
  const [text, setText] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [saved, setSaved] = useState(false);
  const savedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    getLetter()
      .then((l) => setText(l ?? ""))
      .finally(() => setLoaded(true));
    return () => {
      if (savedTimer.current) clearTimeout(savedTimer.current);
    };
  }, []);

  const save = async () => {
    await saveLetter(text);
    setSaved(true);
    if (savedTimer.current) clearTimeout(savedTimer.current);
    savedTimer.current = setTimeout(() => setSaved(false), 2500);
  };

  return (
    <div className="screen">
      <TopBar title="Letter to self" onBack={onBack} />

      <p className="t-body text-text-2">
        This is shown to you during a block, right when it's hardest to remember
        why you started. Speak to your future self plainly.
      </p>

      <textarea
        value={text}
        onChange={(e) => {
          setText(e.target.value);
          setSaved(false);
        }}
        placeholder={PLACEHOLDER}
        spellCheck
        disabled={!loaded}
        className="mt-5 min-h-[220px] w-full resize-y rounded-[14px] border border-hairline bg-surface-1 px-4 py-3 t-body leading-relaxed text-text placeholder:text-text-3 focus:border-accent focus:outline-none"
      />

      <div className="mt-4 flex items-center gap-3">
        <Button variant="primary" onClick={save} disabled={!loaded}>
          Save letter
        </Button>
        {saved && <span className="t-caption text-accent fade-in">Saved.</span>}
      </div>

      <GroupFootnote>
        Stored only on this device. It's part of your protection, so you can edit
        it even during a locked session.
      </GroupFootnote>
    </div>
  );
}

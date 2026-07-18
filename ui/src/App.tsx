import { useCallback, useEffect, useState } from "react";
import { getStatus } from "./lib/ipc";
import type { Status } from "./lib/types";
import Home from "./screens/Home";
import Protection from "./screens/Protection";
import Schedule from "./screens/Schedule";
import BlockList from "./screens/BlockList";
import Progress from "./screens/Progress";
import UrgeOverlay from "./components/UrgeOverlay";

export type Screen = "home" | "protection" | "schedule" | "blocklist" | "progress";

export default function App() {
  const [screen, setScreen] = useState<Screen>("home");
  const [status, setStatus] = useState<Status | null>(null);
  const [urge, setUrge] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await getStatus());
    } catch {
      /* keep the previous status on transient errors */
    }
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 5000);
    return () => clearInterval(t);
  }, [refresh]);

  const back = () => setScreen("home");

  return (
    <div className="flex min-h-full w-full justify-center bg-bg text-text-1">
      <main className="w-full max-w-md px-5 py-8">
        {screen === "home" && (
          <Home status={status} onNavigate={setScreen} onUrge={() => setUrge(true)} />
        )}
        {screen === "protection" && (
          <Protection status={status} onBack={back} refresh={refresh} />
        )}
        {screen === "schedule" && <Schedule status={status} onBack={back} refresh={refresh} />}
        {screen === "blocklist" && (
          <BlockList status={status} onBack={back} refresh={refresh} />
        )}
        {screen === "progress" && (
          <Progress status={status} onBack={back} refresh={refresh} />
        )}
      </main>

      {urge && <UrgeOverlay onClose={() => setUrge(false)} />}
    </div>
  );
}

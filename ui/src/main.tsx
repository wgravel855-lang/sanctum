import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import InterventionWindow from "./screens/InterventionWindow";
// One self-hosted variable family (bundled by Vite — no external CDN, works
// offline under the Tauri CSP). Inter Tight, tuned to read like SF.
import "@fontsource-variable/inter-tight";
import "./styles.css";

// The always-on-top intervention window loads the same bundle at
// `index.html#intervention` (v0.1.5 §B); everything else is the main app.
const isIntervention = window.location.hash.replace(/^#\/?/, "").startsWith("intervention");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isIntervention ? <InterventionWindow /> : <App />}</React.StrictMode>,
);

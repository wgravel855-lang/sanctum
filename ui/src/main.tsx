import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// One self-hosted variable family (bundled by Vite — no external CDN, works
// offline under the Tauri CSP). Inter Tight, tuned to read like SF.
import "@fontsource-variable/inter-tight";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// Self-hosted variable fonts (bundled by Vite — no external CDN, works offline
// under the Tauri CSP). Fraunces = calm editorial display; Inter = UI/body.
import "@fontsource-variable/fraunces";
import "@fontsource-variable/inter";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

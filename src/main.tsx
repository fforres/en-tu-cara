import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./global.css";
import { runStartupUpdateCheck } from "./lib/updater";

// Self-update once per launch, from the always-loaded tray window only (avoid
// N parallel checks across overlay/settings webviews). A few seconds after boot
// so it never competes with the first calendar fetch. See src/lib/updater.ts +
// docs/RELEASING.md.
if (!import.meta.env.DEV) {
  const isTray = !new URLSearchParams(window.location.search).get("window");
  if (isTray) {
    setTimeout(() => void runStartupUpdateCheck(), 8000);
  }
}

// No browser context menu anywhere — right-click → "Reload" on a fullscreen
// alarm is nonsense (CP1b-human feedback). Keep it in dev for devtools access.
if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (e) => e.preventDefault());
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

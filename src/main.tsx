import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./global.css";
import { notifyIfUpdated, runStartupUpdateCheck } from "./lib/updater";

// Self-update once per launch, from the dedicated hidden `background` window
// only (avoid N parallel checks across the overlay/settings webviews). First
// announce if we just updated (running vs last-recorded version), then a few
// seconds after boot check for the next update so it never competes with the
// first calendar fetch. See src/lib/updater.ts + docs/RELEASING.md.
if (!import.meta.env.DEV) {
  const isBackground = new URLSearchParams(window.location.search).get("window") === "background";
  if (isBackground) {
    void notifyIfUpdated();
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

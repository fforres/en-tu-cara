import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./global.css";
import { invoke } from "@tauri-apps/api/core";
import { notifyIfUpdated, runStartupUpdateCheck } from "./lib/updater";

// Startup chores run from the dedicated hidden `background` window only (so they
// happen once, not per overlay/settings webview). See src/lib/updater.ts +
// docs/RELEASING.md.
{
  const isBackground = new URLSearchParams(window.location.search).get("window") === "background";
  if (isBackground) {
    // First run → show the onboarding window (welcome + permission requests).
    // Dev + prod (the dev binary has its own TCC identity to grant).
    void invoke("maybe_show_onboarding");
    // Self-update only in release builds.
    if (!import.meta.env.DEV) {
      void notifyIfUpdated();
      setTimeout(() => void runStartupUpdateCheck(), 8000);
    }
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

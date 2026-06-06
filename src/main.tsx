import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./global.css";

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

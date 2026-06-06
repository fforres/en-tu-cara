import { OverlayAlert } from "./windows/overlay/OverlayAlert";
import { SettingsWindow } from "./windows/settings/SettingsWindow";

// One bundle, multiple windows: the Rust side / tauri.conf opens
// index.html?window=<kind>. `overlay` = takeover alert, `settings` = the
// settings window, `background` = the hidden window that only hosts the
// self-update check (see main.tsx) and renders nothing.
export default function App() {
  const kind = new URLSearchParams(window.location.search).get("window");
  if (kind === "overlay") {
    return <OverlayAlert />;
  }
  if (kind === "settings") {
    return <SettingsWindow />;
  }
  return null;
}

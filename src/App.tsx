import { TrayPopover } from "./windows/tray/TrayPopover";
import { OverlayAlert } from "./windows/overlay/OverlayAlert";
import { SettingsWindow } from "./windows/settings/SettingsWindow";

// One bundle, multiple windows: the Rust side opens index.html?window=<kind>.
export default function App() {
  const kind = new URLSearchParams(window.location.search).get("window");
  if (kind === "overlay") {
    return <OverlayAlert />;
  }
  if (kind === "settings") {
    return <SettingsWindow />;
  }
  return <TrayPopover />;
}

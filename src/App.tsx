import { TrayPopover } from "./windows/tray/TrayPopover";
import { OverlayAlert } from "./windows/overlay/OverlayAlert";
import { SettingsWindow } from "./windows/settings/SettingsWindow";
import { OnboardingWindow } from "./windows/onboarding/OnboardingWindow";

// One bundle, multiple windows: the Rust side / tauri.conf opens
// index.html?window=<kind>. `popover` = the tray-icon popover (also the always-
// loaded host for the self-update check, see main.tsx); `overlay` = takeover
// alert; `settings` = settings window; `onboarding` = first-run welcome.
export default function App() {
  const kind = new URLSearchParams(window.location.search).get("window");
  if (kind === "overlay") {
    return <OverlayAlert />;
  }
  if (kind === "settings") {
    return <SettingsWindow />;
  }
  if (kind === "onboarding") {
    return <OnboardingWindow />;
  }
  if (kind === "popover") {
    return <TrayPopover />;
  }
  return null;
}

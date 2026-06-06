import { TrayPopover } from "./windows/tray/TrayPopover";

// Phase 0: the only window is the tray popover. Overlay and settings windows
// register their own entry points in later phases (PLAN §1 architecture).
export default function App() {
  return <TrayPopover />;
}

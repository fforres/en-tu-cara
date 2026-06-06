// First-launch permission requests. macOS only shows a permission dialog when
// the app actually asks — so we ask on startup (from the hidden background
// window; the dialogs are app-level and appear regardless of window visibility):
//
//   • Calendar (EventKit) — only when the user hasn't decided yet
//     (NotDetermined); once granted/denied macOS won't prompt again.
//   • Notifications — used for the "updated" toast and (later) alerts.
//
// "Run at login / in the background" needs no dialog: it's a LaunchAgent
// registered from Rust per settings.launch_at_login (default on) — see lib.rs.
import { invoke } from "@tauri-apps/api/core";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";

export async function requestStartupPermissions(): Promise<void> {
  // Calendar: prompt only if the user hasn't answered yet.
  try {
    const status = await invoke<string>("calendar_authorization_status");
    if (status === "NotDetermined") {
      await invoke<boolean>("request_calendar_access");
    }
  } catch (error) {
    console.warn("[permissions] calendar request failed:", error);
  }

  // Notifications: requestPermission is a no-op prompt once already decided.
  try {
    if (!(await isPermissionGranted())) {
      await requestPermission();
    }
  } catch (error) {
    console.warn("[permissions] notification request failed:", error);
  }
}

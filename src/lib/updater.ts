// Self-update + a visible notification when it happens. The Rust updater plugin
// checks the `latest.json` published on each GitHub release (see tauri.conf.json
// → plugins.updater.endpoints), verifies the bundle's minisign signature against
// the embedded pubkey, swaps the .app in place, and process::relaunch() restarts
// into the new version.
//
// Two notifications make a deploy observable end-to-end:
//   1. "Installing v<next>…" — fired by the OLD version when it finds an update.
//   2. "Updated to v<current>" — fired by the NEW version on its first launch,
//      by comparing the running version against the last version we recorded.
// (2) requires the previous version to have recorded its version, so this code
// must ship in the version you update *from*.
//
// Distribution: see docs/RELEASING.md.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { load } from "@tauri-apps/plugin-store";

// Persisted in app_data_dir (survives the .app swap, unlike anything in-bundle).
const STORE_FILE = "update-state.json";
const LAST_VERSION_KEY = "lastRunVersion";

async function ensureNotificationPermission(): Promise<boolean> {
  if (await isPermissionGranted()) {
    return true;
  }
  return (await requestPermission()) === "granted";
}

async function notify(title: string, body: string): Promise<void> {
  try {
    if (await ensureNotificationPermission()) {
      sendNotification({ title, body });
    }
  } catch (error) {
    console.warn("[updater] notification failed:", error);
  }
}

/**
 * If this launch runs a different version than the last launch recorded, the app
 * was just updated — notify the user. Records the current version for next time.
 * Call once on startup (tray window). __APP_VERSION__ is injected by Vite.
 */
export async function notifyIfUpdated(): Promise<void> {
  const current = __APP_VERSION__;
  try {
    // Pre-authorize now so the post-update notification can actually show.
    await ensureNotificationPermission();
    const store = await load(STORE_FILE, { autoSave: true, defaults: {} });
    const last = await store.get<string>(LAST_VERSION_KEY);
    if (last && last !== current) {
      await notify("En Tu Cara updated", `Now running v${current} (was v${last}).`);
    }
    await store.set(LAST_VERSION_KEY, current);
  } catch (error) {
    console.warn("[updater] version-check failed:", error);
  }
}

export type UpdateOutcome =
  | { status: "none" }
  | { status: "available"; version: string; update: Update }
  | { status: "error"; error: unknown };

/** Check for an update without installing. Returns the handle if one exists. */
export async function checkForUpdate(): Promise<UpdateOutcome> {
  try {
    const update = await check();
    if (!update) {
      return { status: "none" };
    }
    return { status: "available", version: update.version, update };
  } catch (error) {
    return { status: "error", error };
  }
}

/** Download + install the given update, then relaunch into it. */
export async function installAndRelaunch(update: Update): Promise<void> {
  await update.downloadAndInstall();
  await relaunch();
}

/**
 * Fire-and-forget launch check. If an update exists we notify, install it, and
 * relaunch. Set `auto: false` to only notify (useful while the app is still
 * unsigned — a forced relaunch into an un-notarized bundle can hit Gatekeeper).
 */
export async function runStartupUpdateCheck(auto = true): Promise<void> {
  const result = await checkForUpdate();
  if (result.status === "error") {
    console.warn("[updater] check failed:", result.error);
    return;
  }
  if (result.status === "none") {
    console.info("[updater] up to date");
    return;
  }
  console.info(`[updater] update available: ${result.version}`);
  // Tell the user before we swap + relaunch, so there's a visible signal even
  // when the relaunch is quick.
  await notify("Updating En Tu Cara", `Installing v${result.version}…`);
  if (!auto) {
    return;
  }
  try {
    await installAndRelaunch(result.update);
  } catch (error) {
    console.warn("[updater] install failed:", error);
  }
}

// Self-update. The Rust updater plugin checks the `latest.json` published on
// each GitHub release (see tauri.conf.json → plugins.updater.endpoints),
// verifies the bundle's minisign signature against the embedded pubkey, swaps
// the .app in place, and process::relaunch() restarts into the new version.
//
// Distribution: see docs/RELEASING.md. `pnpm release <patch|minor|major>`
// tags a version; CI (.github/workflows/release.yml) builds + publishes.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

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
 * Fire-and-forget launch check. If an update exists we install it and relaunch.
 * Set `auto: false` to only log availability (useful while the app is still
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
  if (!auto) {
    return;
  }
  try {
    await installAndRelaunch(result.update);
  } catch (error) {
    console.warn("[updater] install failed:", error);
  }
}

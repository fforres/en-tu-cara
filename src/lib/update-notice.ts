// Pure helper for the self-update notification, split out so it can be unit
// tested without importing the Tauri updater/notification/store plugins.
// See src/lib/updater.ts (notifyIfUpdated).

export interface Notice {
  title: string;
  body: string;
}

/**
 * The "you were just updated" notice, or null when this launch is NOT an update
 * — i.e. a fresh install (no recorded prior version) or the same version as last
 * launch. `last` is the version recorded on the previous launch.
 */
export function updatedNotice(last: string | null | undefined, current: string): Notice | null {
  if (!last || last === current) {
    return null;
  }
  return {
    title: "En Tu Cara updated",
    body: `Now running v${current} (was v${last}).`,
  };
}

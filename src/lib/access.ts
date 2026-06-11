// Calendar-access health — the SHARED contract between the Rust access machine
// and every webview banner.
//
// The Rust side (scheduler::get_access_state + the access-state-changed event)
// emits this exact shape; the tray popover and the Settings window both branch
// their banner copy/CTA on `reason`. The contract lives here once so a typo in
// one component can't silently show the wrong banner during an outage — the
// exact failure mode the banners exist to prevent.

/** Payload of `get_access_state` and the `access-state-changed` event. */
export interface AccessStatePayload {
  state: "ok" | "lost";
  /** Loss-mode tag — only meaningful when state is "lost". May be absent on
   * older emissions; treat missing as "" (the generic re-grant branch). */
  reason?: string;
}

/**
 * The one loss mode the app repairs ITSELF (granted, but macOS returns no
 * events — a poisoned TCC record): banners show "repairing" copy + a Repair
 * CTA. Every other reason means the grant is gone and the USER must re-grant.
 * Mirrors access::REASON_FETCH_FAILED in Rust.
 */
export const REASON_FETCH_FAILED = "fetch_failed_despite_authorized";

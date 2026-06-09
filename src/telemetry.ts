// Frontend telemetry → PostHog. Mirror of src-tauri/src/telemetry.rs: anonymized,
// opt-out, device-scoped. Rust is the single source of truth — we ask it (via the
// `telemetry_config` command) whether telemetry is enabled and which distinct_id
// to use, so JS and Rust events unify on ONE device. We never identify a person
// and mark events anonymous (no PostHog person profiles).
//
// Privacy: only behavioral data leaves the webview — never event titles, emails,
// or calendar names. Telemetry must never throw into app code: every entry point
// is wrapped so a telemetry failure can't break a click handler.

import posthog from "posthog-js";
import { invoke } from "@tauri-apps/api/core";

/** Shape returned by the Rust `telemetry_config` command. */
export interface TelemetryConfig {
  enabled: boolean;
  distinct_id: string;
  posthog_key: string;
  api_host: string;
  app_version: string;
}

/** Which webview emitted an event (default tray popover has no `window` param). */
export function currentView(): string {
  return new URLSearchParams(window.location.search).get("window") ?? "popover";
}

/**
 * PostHog init options. Pure (depends only on `config`) so it's unit-tested.
 * Everything that makes sense for a desktop menu-bar app is OFF: no autocapture
 * (no real DOM pages/links to track), no pageviews, no session recording (a
 * fullscreen alarm is not something to replay). `person_profiles: identified_only`
 * + never calling identify() keeps every event anonymous. `bootstrap.distinctID`
 * pins the device UUID so these events share identity with the Rust events.
 */
export function posthogInitOptions(config: TelemetryConfig): Parameters<typeof posthog.init>[1] {
  return {
    api_host: config.api_host,
    person_profiles: "identified_only",
    autocapture: false,
    capture_pageview: false,
    capture_pageleave: false,
    disable_session_recording: true,
    bootstrap: { distinctID: config.distinct_id },
  };
}

let initialized = false;

/**
 * Initialize telemetry once per webview load. Idempotent and safe to call from
 * multiple components. No-ops (and stays uninitialized) when Rust reports
 * telemetry disabled or the config can't be read — so `capture()` stays inert.
 */
export async function initTelemetry(): Promise<void> {
  if (initialized) {
    return;
  }
  let config: TelemetryConfig | undefined;
  try {
    config = await invoke<TelemetryConfig>("telemetry_config");
  } catch {
    return; // command unavailable (e.g. non-Tauri test host) — stay silent
  }
  if (!config?.enabled) {
    return;
  }
  try {
    posthog.init(config.posthog_key, posthogInitOptions(config));
    initialized = true;
    installErrorForwarding();
    capture("app_view_loaded", { view: currentView() });
  } catch {
    initialized = false;
  }
}

/**
 * Capture an event. No-op until initialized, and never throws — telemetry must
 * not be able to break the code path that called it.
 */
export function capture(event: string, props?: Record<string, unknown>): void {
  if (!initialized) {
    return;
  }
  try {
    posthog.capture(event, props);
  } catch {
    // swallow — telemetry is best-effort
  }
}

/**
 * Live-apply a telemetry toggle change from Settings (no restart). Turning on
 * lazily initializes; turning off records the opt-out, stops PostHog capturing,
 * and makes `capture()` inert for the rest of the session.
 */
export async function setTelemetryEnabled(enabled: boolean): Promise<void> {
  if (enabled) {
    await initTelemetry();
    capture("telemetry_toggled", { enabled: true });
    return;
  }
  capture("telemetry_toggled", { enabled: false }); // recorded while still on
  try {
    posthog.opt_out_capturing();
  } catch {
    /* best-effort */
  }
  initialized = false;
}

/** Phase-1 "logs from JS": forward genuine runtime errors as PostHog exceptions. */
function installErrorForwarding(): void {
  window.addEventListener("error", (e) => {
    try {
      posthog.captureException(e.error ?? new Error(e.message));
    } catch {
      /* best-effort */
    }
  });
  window.addEventListener("unhandledrejection", (e) => {
    try {
      const reason = e.reason instanceof Error ? e.reason : new Error(String(e.reason));
      posthog.captureException(reason);
    } catch {
      /* best-effort */
    }
  });
}

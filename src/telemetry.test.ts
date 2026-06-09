import { beforeEach, describe, expect, it, vi } from "vitest";
import { posthogInitOptions, type TelemetryConfig } from "./telemetry";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const posthogMock = vi.hoisted(() => ({
  init: vi.fn(),
  capture: vi.fn(),
  captureException: vi.fn(),
  opt_out_capturing: vi.fn(),
}));
vi.mock("posthog-js", () => ({ default: posthogMock }));

const CONFIG: TelemetryConfig = {
  enabled: true,
  distinct_id: "dev-9",
  posthog_key: "phc_test",
  api_host: "https://us.i.posthog.com",
  app_version: "0.4.4",
};

describe("posthogInitOptions", () => {
  it("turns off everything inappropriate for a desktop app and pins the device id", () => {
    const opts = posthogInitOptions(CONFIG);
    expect(opts).toMatchObject({
      api_host: "https://us.i.posthog.com",
      person_profiles: "identified_only", // anonymous; never identified
      autocapture: false,
      capture_pageview: false,
      capture_pageleave: false,
      disable_session_recording: true,
      bootstrap: { distinctID: "dev-9" },
    });
  });
});

describe("initTelemetry gating", () => {
  beforeEach(() => {
    vi.resetModules(); // fresh `initialized` state per test
    invokeMock.mockReset();
    posthogMock.init.mockReset();
    posthogMock.capture.mockReset();
  });

  it("does not initialize PostHog when Rust reports telemetry disabled", async () => {
    invokeMock.mockResolvedValue({ ...CONFIG, enabled: false });
    const mod = await import("./telemetry");
    await mod.initTelemetry();
    expect(posthogMock.init).not.toHaveBeenCalled();
    mod.capture("alarm_joined"); // must be a safe no-op, not a throw
    expect(posthogMock.capture).not.toHaveBeenCalled();
  });

  it("initializes once and then captures when enabled", async () => {
    invokeMock.mockResolvedValue(CONFIG);
    const mod = await import("./telemetry");
    await mod.initTelemetry();
    expect(posthogMock.init).toHaveBeenCalledWith(
      "phc_test",
      expect.objectContaining({ bootstrap: { distinctID: "dev-9" } }),
    );
    mod.capture("alarm_joined");
    expect(posthogMock.capture).toHaveBeenCalledWith("alarm_joined", undefined);
  });

  it("stays inert (no init, no throw) when the config command is unavailable", async () => {
    invokeMock.mockRejectedValue(new Error("not a tauri host"));
    const mod = await import("./telemetry");
    await mod.initTelemetry();
    expect(posthogMock.init).not.toHaveBeenCalled();
  });
});

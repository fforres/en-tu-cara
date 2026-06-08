import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

const invokeMock = vi.hoisted(() => vi.fn());
const isPermissionGrantedMock = vi.hoisted(() => vi.fn(() => Promise.resolve(false)));
const requestPermissionMock = vi.hoisted(() => vi.fn(() => Promise.resolve("default")));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: isPermissionGrantedMock,
  requestPermission: requestPermissionMock,
}));

import { OnboardingWindow } from "./OnboardingWindow";

// Mutable backend state the mocks read, so a test can flip access mid-run.
let calAuth = "NotDetermined";

beforeEach(() => {
  calAuth = "NotDetermined";
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "calendar_authorization_status") {
      return Promise.resolve(calAuth);
    }
    if (cmd === "get_settings") {
      return Promise.resolve({ launch_at_login: true });
    }
    return Promise.resolve(undefined);
  });
  isPermissionGrantedMock.mockResolvedValue(false);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("OnboardingWindow — permission polling", () => {
  it("flips Calendar to Granted on its own ~5s after access is granted out-of-band", async () => {
    vi.useFakeTimers();
    render(<OnboardingWindow />);

    // Initial check: NotDetermined → the row shows a "Grant access" button.
    await vi.advanceTimersByTimeAsync(0); // flush mount-effect promises
    expect(screen.getByRole("button", { name: "Grant access" })).toBeInTheDocument();

    // User grants in the macOS prompt; the backend now reports full access — but
    // nothing pushed that to the webview. The 5s poll must pick it up.
    calAuth = "FullAccess";
    await vi.advanceTimersByTimeAsync(5000);

    expect(screen.queryByRole("button", { name: "Grant access" })).not.toBeInTheDocument();
    expect(screen.getByText("✓ Granted")).toBeInTheDocument();
  });

  it("stops polling once the window is hidden", async () => {
    vi.useFakeTimers();
    render(<OnboardingWindow />);
    await vi.advanceTimersByTimeAsync(0);
    const callsAfterMount = invokeMock.mock.calls.filter(
      (c) => c[0] === "calendar_authorization_status",
    ).length;

    // Hide the window → the interval should be cleared, so no further polling.
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(15000);

    const callsAfterHidden = invokeMock.mock.calls.filter(
      (c) => c[0] === "calendar_authorization_status",
    ).length;
    expect(callsAfterHidden).toBe(callsAfterMount);

    // Restore for other tests.
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
  });
});

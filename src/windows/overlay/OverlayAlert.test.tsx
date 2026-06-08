import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.hoisted(() => vi.fn());
const listeners = vi.hoisted(() => new Map<string, (e: { payload: unknown }) => void>());
const listenMock = vi.hoisted(
  () =>
    (event: string, cb: (e: { payload: unknown }) => void): Promise<() => void> => {
      listeners.set(event, cb);
      return Promise.resolve(() => listeners.delete(event));
    },
);
const openUrlMock = vi.hoisted(() => vi.fn(() => Promise.resolve()));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));

import { OverlayAlert } from "./OverlayAlert";

const ALARM_A = {
  occurrence_key: "(A @ t)",
  kind: "t_zero",
  title: "Standup",
  start: null,
  end: null,
};
const ALARM_B = { occurrence_key: "(B @ t)", kind: "t_zero", title: "1:1", start: null, end: null };

beforeEach(() => {
  invokeMock.mockReset();
  listeners.clear();
  openUrlMock.mockReset();
  openUrlMock.mockResolvedValue(undefined);
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "get_active_alarms") {
      return Promise.resolve([ALARM_A, ALARM_B]);
    }
    if (cmd === "fetch_events") {
      return Promise.resolve([]);
    }
    if (cmd === "get_settings") {
      return Promise.resolve({ theme: "frost-dark", snooze_minutes: [1, 5] });
    }
    return Promise.resolve(undefined);
  });
});

describe("OverlayAlert — per-occurrence dismiss (overlapping meetings)", () => {
  it("renders one card per active alarm", async () => {
    render(<OverlayAlert />);
    expect(await screen.findByText("Standup")).toBeInTheDocument();
    expect(screen.getByText("1:1")).toBeInTheDocument();
  });

  it("dismissing one card sends THAT occurrence_key, not a blanket dismiss-all", async () => {
    render(<OverlayAlert />);
    await screen.findByText("Standup");
    // Two cards (A then B), each with its own Dismiss; click A's.
    fireEvent.click(screen.getAllByText("Dismiss")[0]);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("dismiss_alarms", { occurrenceKey: "(A @ t)" }),
    );
    // The blanket form (no args) must NOT be used for a single-card dismiss.
    expect(invokeMock).not.toHaveBeenCalledWith("dismiss_alarms");
  });

  it("an alarms-updated event replaces the card set (overlay stays open with the rest)", async () => {
    render(<OverlayAlert />);
    await screen.findByText("Standup");
    // Backend dropped A but kept B: the overlay must re-render with only B.
    await act(async () => {
      listeners.get("alarms-updated")?.({ payload: [ALARM_B] });
    });
    await waitFor(() => expect(screen.queryByText("Standup")).not.toBeInTheDocument());
    expect(screen.getByText("1:1")).toBeInTheDocument();
  });

  it("focuses Dismiss (never Join) so a stray Enter cannot join a meeting", async () => {
    render(<OverlayAlert />);
    await screen.findByText("Standup");
    await waitFor(() => {
      const focused = document.activeElement as HTMLElement | null;
      expect(focused?.getAttributeNames?.()).toContain("data-dismiss");
    });
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.hoisted(() => vi.fn());
const openUrlMock = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));

import { TrayPopover, type UiEvent } from "./TrayPopover";

const ZOOM = "https://us04web.zoom.us/j/123456789";

function futureEvent(): UiEvent {
  return {
    occurrence_key: "(evt @ t1)",
    title: "Design sync",
    start: new Date(Date.now() + 30 * 60_000).toISOString(),
    end: new Date(Date.now() + 90 * 60_000).toISOString(),
    all_day: false,
    status: "confirmed",
    my_rsvp: "accepted",
    is_recurring_occurrence: false,
    calendar_title: "Work",
    calendar_id: "work",
    url: ZOOM,
    location: null,
    notes: null,
  };
}

// Mutable backend ignore-set the mock reads, so the tray reflects ignores.
let ignoredSet: string[] = [];

beforeEach(() => {
  ignoredSet = [];
  invokeMock.mockReset();
  openUrlMock.mockReset();
  openUrlMock.mockResolvedValue(undefined);
  invokeMock.mockImplementation((cmd: string, args?: { occurrenceKey?: string }) => {
    switch (cmd) {
      case "fetch_events":
        return Promise.resolve([futureEvent()]);
      case "list_calendars":
        return Promise.resolve([
          { id: "work", title: "Work", account: "me", color: [0.2, 0.4, 1, 1] },
        ]);
      case "get_paused":
        return Promise.resolve(false);
      case "get_ignored":
        return Promise.resolve([...ignoredSet]);
      case "ignore_occurrence":
        if (args?.occurrenceKey) {
          ignoredSet.push(args.occurrenceKey);
        }
        return Promise.resolve(undefined);
      case "unignore_occurrence":
        ignoredSet = ignoredSet.filter((k) => k !== args?.occurrenceKey);
        return Promise.resolve(undefined);
      default:
        return Promise.resolve(undefined);
    }
  });
});

describe("TrayPopover", () => {
  it("renders the header", () => {
    render(<TrayPopover />);
    expect(screen.getByText("En Tu Cara")).toBeInTheDocument();
  });

  it("'Go to event' button opens the meeting link", async () => {
    render(<TrayPopover />);
    fireEvent.click(await screen.findByRole("button", { name: "Go to event" }));
    await waitFor(() => expect(openUrlMock).toHaveBeenCalledWith(ZOOM));
  });

  it("right-click → Ignore sends ignore_occurrence for THAT occurrence and dims it", async () => {
    render(<TrayPopover />);
    fireEvent.contextMenu(await screen.findByText("Design sync"));
    fireEvent.click(await screen.findByRole("button", { name: "Ignore this event" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("ignore_occurrence", { occurrenceKey: "(evt @ t1)" }),
    );
    expect(await screen.findByText("IGNORED")).toBeInTheDocument();
  });

  it("right-click an already-ignored event offers Stop ignoring", async () => {
    ignoredSet = ["(evt @ t1)"];
    render(<TrayPopover />);
    fireEvent.contextMenu(await screen.findByText("Design sync"));
    fireEvent.click(await screen.findByRole("button", { name: "Stop ignoring this event" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("unignore_occurrence", {
        occurrenceKey: "(evt @ t1)",
      }),
    );
  });

  it("right-click → Open in browser opens the link", async () => {
    render(<TrayPopover />);
    fireEvent.contextMenu(await screen.findByText("Design sync"));
    fireEvent.click(await screen.findByRole("button", { name: "Open in browser" }));
    await waitFor(() => expect(openUrlMock).toHaveBeenCalledWith(ZOOM));
  });

  it("right-click → Copy link writes the URL to the clipboard", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<TrayPopover />);
    fireEvent.contextMenu(await screen.findByText("Design sync"));
    fireEvent.click(await screen.findByRole("button", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(ZOOM));
  });

  it("shows the calendar origin (account · calendar) instead of the raw link", async () => {
    render(<TrayPopover />);
    expect(await screen.findByText("me · Work")).toBeInTheDocument();
    // The raw URL is no longer shown as a text line.
    expect(screen.queryByText(ZOOM)).not.toBeInTheDocument();
  });
});

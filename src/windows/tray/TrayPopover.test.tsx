import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.hoisted(() => vi.fn());
const openUrlMock = vi.hoisted(() => vi.fn(() => Promise.resolve()));
const listeners = vi.hoisted(() => new Map<string, (e: { payload: unknown }) => void>());
const listenMock = vi.hoisted(() => (name: string, cb: (e: { payload: unknown }) => void) => {
  listeners.set(name, cb);
  return Promise.resolve(() => listeners.delete(name));
});
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: openUrlMock }));

import { TrayPopover, type UiEvent } from "./TrayPopover";

const ZOOM = "https://us04web.zoom.us/j/123456789";
const WEB_EVENT = "https://calendar.google.com/calendar/event?eid=abc123";

function futureEvent(): UiEvent {
  return {
    id: "EK-EVENT-ID",
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
    // url = the web "view event" link (EKEvent.URL); the videocall/join link
    // lives in notes here so the two are DISTINCT and the split is testable.
    url: WEB_EVENT,
    location: null,
    notes: `Join: ${ZOOM}`,
    calendars: [{ calendar_id: "work", calendar_title: "Work" }],
  };
}

// Same meeting present on two accounts (business + personal) — the deduped shape
// the backend now returns: one row carrying both calendars.
function dupedEvent(): UiEvent {
  return {
    ...futureEvent(),
    title: "ADHD meds",
    occurrence_key: "(adhd @ t1)",
    calendar_id: "work",
    calendar_title: "Felipe Torres Gmail",
    url: null,
    notes: null,
    calendars: [
      { calendar_id: "work", calendar_title: "Felipe Torres Gmail" },
      { calendar_id: "personal", calendar_title: "Felipe Torres Gmail" },
    ],
  };
}

// Mutable backend ignore-set the mock reads, so the tray reflects ignores.
let ignoredSet: string[] = [];

beforeEach(() => {
  ignoredSet = [];
  listeners.clear();
  invokeMock.mockReset();
  openUrlMock.mockReset();
  openUrlMock.mockResolvedValue(undefined);
  invokeMock.mockImplementation((cmd: string, args?: { occurrenceKey?: string }) => {
    switch (cmd) {
      case "refresh_popover":
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

  it("shows the access-lost banner but KEEPS showing the (stale) events", async () => {
    render(<TrayPopover />);
    // Events render normally first.
    expect(await screen.findByText("Design sync")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    // A live access-lost edge raises the banner — without removing the events
    // (preserve-on-failure: last-known events stay, flagged as maybe-outdated).
    listeners.get("access-state-changed")?.({ payload: { state: "lost" } });
    const banner = await screen.findByRole("alert");
    expect(banner).toHaveTextContent(/may be outdated/i);
    expect(screen.getByText("Design sync")).toBeInTheDocument();
    // Recovery clears the banner.
    listeners.get("access-state-changed")?.({ payload: { state: "ok" } });
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("'Open videocall' button opens the videocall/join link (not the web event)", async () => {
    render(<TrayPopover />);
    fireEvent.click(await screen.findByRole("button", { name: "Open videocall" }));
    await waitFor(() => expect(openUrlMock).toHaveBeenCalledWith(ZOOM));
    expect(openUrlMock).not.toHaveBeenCalledWith(WEB_EVENT);
  });

  it("left-clicking the row opens the web calendar event (event.url), not the videocall", async () => {
    render(<TrayPopover />);
    fireEvent.click(await screen.findByText("Design sync"));
    await waitFor(() => expect(openUrlMock).toHaveBeenCalledWith(WEB_EVENT));
    expect(openUrlMock).not.toHaveBeenCalledWith(ZOOM);
  });

  it("right-click → Ignore sends ignore_occurrence for THAT occurrence (with its end) and dims it", async () => {
    render(<TrayPopover />);
    fireEvent.contextMenu(await screen.findByText("Design sync"));
    fireEvent.click(await screen.findByRole("button", { name: "Ignore this event" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "ignore_occurrence",
        expect.objectContaining({ occurrenceKey: "(evt @ t1)", endsAt: expect.any(String) }),
      ),
    );
    expect(await screen.findByText("IGNORED")).toBeInTheDocument();
  });

  it("reverts the optimistic IGNORED state when ignore_occurrence rejects", async () => {
    // Bug H3: a failed backend write left the row showing IGNORED while the
    // alarm would still fire. The optimistic flip must revert on rejection.
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "refresh_popover":
          return Promise.resolve([futureEvent()]);
        case "list_calendars":
          return Promise.resolve([
            { id: "work", title: "Work", account: "me", color: [0.2, 0.4, 1, 1] },
          ]);
        case "get_paused":
          return Promise.resolve(false);
        case "get_ignored":
          return Promise.resolve([] as string[]);
        case "ignore_occurrence":
          return Promise.reject(new Error("backend write failed"));
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<TrayPopover />);
    fireEvent.contextMenu(await screen.findByText("Design sync"));
    fireEvent.click(await screen.findByRole("button", { name: "Ignore this event" }));
    // Optimistically shows IGNORED, then reverts once the write rejects.
    await waitFor(() => expect(screen.queryByText("IGNORED")).not.toBeInTheDocument());
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

  it("right-click → Open in local calendar invokes open_in_calendar with the event id", async () => {
    render(<TrayPopover />);
    fireEvent.contextMenu(await screen.findByText("Design sync"));
    fireEvent.click(await screen.findByRole("button", { name: "Open in local calendar" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("open_in_calendar", { eventId: "EK-EVENT-ID" }),
    );
  });

  it("keeps the previously-shown events when a later refresh_popover fails (no clobber)", async () => {
    // Regression: a transient EventKit blip must NOT clear the visible list.
    // First refresh succeeds; a later one rejects → the event must stay.
    let calls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "refresh_popover":
          calls += 1;
          return calls === 1
            ? Promise.resolve([futureEvent()])
            : Promise.reject(new Error("EventKit blip"));
        case "list_calendars":
          return Promise.resolve([
            { id: "work", title: "Work", account: "me", color: [0.2, 0.4, 1, 1] },
          ]);
        case "get_paused":
          return Promise.resolve(false);
        case "get_ignored":
          return Promise.resolve([] as string[]);
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<TrayPopover />);
    expect(await screen.findByText("Design sync")).toBeInTheDocument();
    // Drive another refresh: focus restarts polling and calls refresh() now.
    fireEvent(window, new Event("focus"));
    // Give the rejected refresh a chance to resolve.
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText("Design sync")).toBeInTheDocument();
  });

  it("preserves the pause toggle when a later get_paused fails (no silent un-pause)", async () => {
    // Bug H1: get_paused resolved to `false` on a transient IPC blip and
    // setPaused ran unconditionally — flipping the toggle to "running" even
    // though the backend stayed paused. A failed read must PRESERVE last-good,
    // like the other three reads in refresh().
    let pausedCalls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "refresh_popover":
          return Promise.resolve([futureEvent()]);
        case "list_calendars":
          return Promise.resolve([
            { id: "work", title: "Work", account: "me", color: [0.2, 0.4, 1, 1] },
          ]);
        case "get_paused":
          pausedCalls += 1;
          // First read: paused. Later reads: transient failure.
          return pausedCalls === 1
            ? Promise.resolve(true)
            : Promise.reject(new Error("get_paused blip"));
        case "get_ignored":
          return Promise.resolve([] as string[]);
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<TrayPopover />);
    // First refresh paused → the toggle offers "Resume alerts" (its title).
    expect(await screen.findByTitle("Resume alerts")).toBeInTheDocument();
    // A later refresh whose get_paused REJECTS must keep the paused toggle.
    fireEvent(window, new Event("focus"));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByTitle("Resume alerts")).toBeInTheDocument();
    expect(screen.queryByTitle("Pause alerts")).not.toBeInTheDocument();
  });

  it("shows the calendar origin (account · calendar) instead of the raw link", async () => {
    render(<TrayPopover />);
    expect(await screen.findByText("me · Work")).toBeInTheDocument();
    // The raw URL is no longer shown as a text line.
    expect(screen.queryByText(ZOOM)).not.toBeInTheDocument();
  });

  it("keeps the calendar origin when a later list_calendars fails (no clobber)", async () => {
    // Same preserve-on-failure discipline as refresh_popover: a transient blip in
    // list_calendars must not blank out the "account · calendar" origin line.
    let calls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "refresh_popover":
          return Promise.resolve([futureEvent()]);
        case "list_calendars":
          calls += 1;
          return calls === 1
            ? Promise.resolve([
                { id: "work", title: "Work", account: "me", color: [0.2, 0.4, 1, 1] },
              ])
            : Promise.reject(new Error("list_calendars blip"));
        case "get_paused":
          return Promise.resolve(false);
        case "get_ignored":
          return Promise.resolve([] as string[]);
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<TrayPopover />);
    expect(await screen.findByText("me · Work")).toBeInTheDocument();
    fireEvent(window, new Event("focus"));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText("me · Work")).toBeInTheDocument();
  });

  it("keeps the IGNORED badge when a later get_ignored fails (no clobber)", async () => {
    // A transient get_ignored failure must preserve the last-good ignore set,
    // never silently un-dim an ignored event (which would let it alert again).
    let calls = 0;
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "refresh_popover":
          return Promise.resolve([futureEvent()]);
        case "list_calendars":
          return Promise.resolve([
            { id: "work", title: "Work", account: "me", color: [0.2, 0.4, 1, 1] },
          ]);
        case "get_paused":
          return Promise.resolve(false);
        case "get_ignored":
          calls += 1;
          return calls === 1
            ? Promise.resolve(["(evt @ t1)"])
            : Promise.reject(new Error("get_ignored blip"));
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<TrayPopover />);
    expect(await screen.findByText("IGNORED")).toBeInTheDocument();
    fireEvent(window, new Event("focus"));
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.getByText("IGNORED")).toBeInTheDocument();
  });

  // --- Access banner reason-aware copy tests ---
  //
  // The banner has TWO distinct messages. Showing the wrong one during an outage
  // actively misleads the user: "fetch_failed_despite_authorized" means EventKit
  // is still granted but temporarily unresponsive — events shown ARE a stale
  // snapshot and will self-repair, so users should wait. Any other reason means
  // the grant is gone — events ARE outdated and the user must act. Mixing the two
  // branches causes either false reassurance ("Repairing automatically" when the
  // grant is revoked) or a false alarm ("Open Settings to fix" when self-repair
  // is already running).

  it("mount with state=lost reason=fetch_failed_despite_authorized shows the stale-snapshot copy", async () => {
    // Regression: mount-time get_access_state must populate accessReason before
    // the banner renders, so the "Repairing automatically" branch fires for the
    // self-healing fetch failure — not the generic "Open Settings to fix" copy.
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "refresh_popover":
          return Promise.resolve([futureEvent()]);
        case "list_calendars":
          return Promise.resolve([
            { id: "work", title: "Work", account: "me", color: [0.2, 0.4, 1, 1] },
          ]);
        case "get_paused":
          return Promise.resolve(false);
        case "get_ignored":
          return Promise.resolve([] as string[]);
        case "get_access_state":
          return Promise.resolve({ state: "lost", reason: "fetch_failed_despite_authorized" });
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<TrayPopover />);
    const banner = await screen.findByRole("alert");
    // "Repairing automatically" is the distinctive phrase for the self-healing
    // path; the generic branch never contains it.
    expect(banner).toHaveTextContent(/Repairing automatically/i);
    // Sanity: the generic "Open Settings to fix" copy must NOT appear when the
    // reason is the self-healing one (that would send the user to Settings for
    // something that fixes itself, wasting their time during an outage).
    expect(banner).not.toHaveTextContent(/Open Settings to fix/i);
  });

  it("mount with state=lost reason=authorization_not_determined shows the generic access-lost copy", async () => {
    // Regression: a reason OTHER than fetch_failed_despite_authorized must land
    // on the generic "access lost — these events may be outdated. Open Settings"
    // copy, telling the user the grant is gone and they need to act — not that
    // self-repair is running.
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "refresh_popover":
          return Promise.resolve([futureEvent()]);
        case "list_calendars":
          return Promise.resolve([
            { id: "work", title: "Work", account: "me", color: [0.2, 0.4, 1, 1] },
          ]);
        case "get_paused":
          return Promise.resolve(false);
        case "get_ignored":
          return Promise.resolve([] as string[]);
        case "get_access_state":
          return Promise.resolve({ state: "lost", reason: "authorization_not_determined" });
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<TrayPopover />);
    const banner = await screen.findByRole("alert");
    expect(banner).toHaveTextContent(/may be outdated/i);
    expect(banner).toHaveTextContent(/Open Settings to fix/i);
    // Sanity: "Repairing automatically" must NOT appear — this reason requires
    // the user to act, not wait.
    expect(banner).not.toHaveTextContent(/Repairing automatically/i);
  });

  it("live event ok→lost(fetch_failed) shows stale-snapshot copy; subsequent ok removes the banner", async () => {
    // Regression: the live "access-state-changed" edge must also populate
    // accessReason correctly (not just the mount-time get_access_state call).
    // Without this, a live revocation mid-session shows the wrong copy.
    // Also verifies that a follow-up state=ok event clears the banner entirely,
    // so a recovered session doesn't leave a stale warning on screen.
    render(<TrayPopover />);
    // Start clean — no banner.
    expect(await screen.findByText("Design sync")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();

    // Live edge: access lost with the self-healing reason.
    listeners.get("access-state-changed")?.({
      payload: { state: "lost", reason: "fetch_failed_despite_authorized" },
    });
    const banner = await screen.findByRole("alert");
    expect(banner).toHaveTextContent(/Repairing automatically/i);

    // Recovery edge: state=ok must remove the banner unconditionally.
    listeners.get("access-state-changed")?.({ payload: { state: "ok" } });
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  // --- Cross-account dedup: the tray shows ONE row (the breakdown of accounts
  // lives on the takeover, not here) ---

  it("a deduped cross-account meeting shows as a single row with one origin line", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "refresh_popover":
          return Promise.resolve([dupedEvent()]);
        case "list_calendars":
          return Promise.resolve([
            { id: "work", title: "Felipe Torres Gmail", account: "felipe@skyward.ai", color: null },
            { id: "personal", title: "Felipe Torres Gmail", account: "Google", color: null },
          ]);
        case "get_paused":
          return Promise.resolve(false);
        case "get_ignored":
          return Promise.resolve([] as string[]);
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<TrayPopover />);
    // One row with the survivor's own origin — the OTHER account (Google) is NOT
    // listed in the tray, and there's no duplicate-count badge.
    expect(await screen.findByText("ADHD meds")).toBeInTheDocument();
    expect(screen.getByText("felipe@skyward.ai · Felipe Torres Gmail")).toBeInTheDocument();
    expect(screen.queryByText("Google · Felipe Torres Gmail")).not.toBeInTheDocument();
    expect(screen.queryByText(/⧉/)).not.toBeInTheDocument();
  });

  it("live event with reason=undefined falls back to generic copy without crashing", async () => {
    // Regression: the payload type allows reason to be absent (optional field).
    // accessReason must default to "" so the banner uses the generic branch — not
    // crash or render nothing — when the backend omits reason. A crash here would
    // silently suppress the access warning, hiding it from a user who needs it.
    render(<TrayPopover />);
    expect(await screen.findByText("Design sync")).toBeInTheDocument();

    // Fire a lost event with NO reason field at all.
    listeners.get("access-state-changed")?.({ payload: { state: "lost" } });
    const banner = await screen.findByRole("alert");
    // No reason → falls through to generic copy.
    expect(banner).toHaveTextContent(/may be outdated/i);
    expect(banner).toHaveTextContent(/Open Settings to fix/i);
    expect(banner).not.toHaveTextContent(/Repairing automatically/i);
  });
});

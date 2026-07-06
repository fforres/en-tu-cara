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
      return Promise.resolve({ theme: "frost-dark", default_snooze_minutes: 20 });
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

  it("renders one 'Remind me again' button using the configured default snooze duration", async () => {
    render(<OverlayAlert />);
    await screen.findByText("Standup");
    // One snooze button per card, labelled with the default snooze duration (20).
    const snoozeButtons = await screen.findAllByText("Remind me again in 20 minutes");
    expect(snoozeButtons.length).toBe(2); // one per active alarm card
    // Clicking snoozes THAT occurrence by the configured default (not a hardcoded value).
    fireEvent.click(snoozeButtons[0]);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("snooze_alarm", {
        occurrenceKey: "(A @ t)",
        minutes: 20,
      }),
    );
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

  it("retries fetch_events so a transient blip doesn't permanently strip Join", async () => {
    // M4: the Join link is resolved from a SEPARATE fetch_events (the alarm
    // payload carries no URL). A transient EventKit failure at mount used to
    // leave events empty forever → no Join button, exactly when the user most
    // needs it. The overlay must retry a FAILED read until it succeeds.
    vi.useFakeTimers();
    try {
      const ZOOM = "https://us04web.zoom.us/j/123456789";
      let fetchCalls = 0;
      invokeMock.mockImplementation((cmd: string) => {
        if (cmd === "get_active_alarms") {
          return Promise.resolve([ALARM_A]);
        }
        if (cmd === "fetch_events") {
          fetchCalls += 1;
          // First attempt blips; the retry succeeds with the linked event.
          return fetchCalls === 1
            ? Promise.reject(new Error("EventKit blip"))
            : Promise.resolve([
                {
                  id: "EK-A",
                  occurrence_key: "(A @ t)",
                  title: "Standup",
                  start: null,
                  end: null,
                  all_day: false,
                  status: "confirmed",
                  my_rsvp: "accepted",
                  is_recurring_occurrence: false,
                  calendar_title: "Work",
                  calendar_id: "work",
                  url: null,
                  location: null,
                  notes: `Join: ${ZOOM}`,
                },
              ]);
        }
        if (cmd === "get_settings") {
          return Promise.resolve({ theme: "frost-dark", default_snooze_minutes: 20 });
        }
        return Promise.resolve(undefined);
      });
      render(<OverlayAlert />);
      // First fetch failed → no Join yet.
      await vi.advanceTimersByTimeAsync(0);
      expect(screen.queryByText("📹 Join")).not.toBeInTheDocument();
      // The retry (1s backoff) succeeds → Join appears.
      await vi.advanceTimersByTimeAsync(1000);
      expect(screen.getByText("📹 Join")).toBeInTheDocument();
      expect(fetchCalls).toBeGreaterThanOrEqual(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("scrolls overflowing content with the scrollbar hidden (no clipping)", async () => {
    // Regression: with many/large cards the content overflowed the screen and was
    // clipped (flex-centering made the top unreachable). The takeover must be a
    // scroll container with the scrollbar hidden (trackpad/wheel still scrolls).
    const { container } = render(<OverlayAlert />);
    await screen.findByText("Standup");
    const main = container.querySelector("main");
    expect(main).not.toBeNull();
    expect(main!.style.overflowY).toBe("auto");
    // scrollbarWidth:none (Firefox) + the .overlay-scroll ::-webkit-scrollbar rule
    // (WebKit, the takeover's engine) keep the bar hidden.
    expect(main!.style.scrollbarWidth).toBe("none");
    expect(main!.className).toContain("overlay-scroll");
  });

  it("re-lands focus on Dismiss when the card set swaps without changing count", async () => {
    render(<OverlayAlert />);
    await screen.findByText("Standup");
    // Park focus on B's Dismiss (the card about to be removed), simulating the
    // user having tabbed to it. The component's "don't steal focus if the user is
    // already on one of our buttons" guard means it leaves this alone.
    const dismissButtons = screen.getAllByText("Dismiss");
    act(() => dismissButtons[dismissButtons.length - 1].focus());
    // Swap the set without changing the count: [A,B] -> [A,C]. Keyed on
    // cards.length the focus effect wouldn't re-run, so focus strands on B's
    // removed Dismiss node (activeElement falls back to <body>, no data-dismiss).
    // Keyed on card identity the effect re-runs and re-lands focus on a live one.
    const ALARM_C = {
      occurrence_key: "(C @ t)",
      kind: "t_zero",
      title: "Retro",
      start: null,
      end: null,
    };
    await act(async () => {
      listeners.get("alarms-updated")?.({ payload: [ALARM_A, ALARM_C] });
    });
    await screen.findByText("Retro");
    expect(screen.queryByText("1:1")).not.toBeInTheDocument();
    await waitFor(() => {
      const focused = document.activeElement as HTMLElement | null;
      // Focus must be on a LIVE Dismiss button (one still attached to the document).
      expect(focused?.getAttributeNames?.()).toContain("data-dismiss");
      expect(document.body.contains(focused)).toBe(true);
    });
  });
});

describe("OverlayAlert — calendar origins (where the event came from)", () => {
  // An event carrying the calendars it was deduped from, matched to the alarm by
  // occurrence_key. `account` lives on the calendar (resolved via list_calendars).
  const eventOn = (calendars: { calendar_id: string; calendar_title: string }[]) => ({
    id: "EK",
    occurrence_key: ALARM_A.occurrence_key,
    title: ALARM_A.title,
    start: null,
    end: null,
    all_day: false,
    status: "confirmed",
    my_rsvp: "accepted",
    is_recurring_occurrence: false,
    calendar_title: calendars[0]?.calendar_title ?? null,
    calendar_id: calendars[0]?.calendar_id ?? null,
    url: null,
    location: null,
    notes: null,
    calendars,
  });

  const mockBackend = (
    calendars: { calendar_id: string; calendar_title: string }[],
    calendarList: { id: string; account: string | null }[],
  ) => {
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "get_active_alarms":
          return Promise.resolve([ALARM_A]);
        case "fetch_events":
          return Promise.resolve([eventOn(calendars)]);
        case "list_calendars":
          return Promise.resolve(calendarList);
        case "get_settings":
          return Promise.resolve({ theme: "frost-dark", default_snooze_minutes: 20 });
        default:
          return Promise.resolve(undefined);
      }
    });
  };

  it("lists each distinct synced account for a genuine cross-account duplicate", async () => {
    mockBackend(
      [
        { calendar_id: "g", calendar_title: "FELIPE TORRES — GMAIL" },
        { calendar_id: "s", calendar_title: "FELIPE TORRES — GMAIL" },
        { calendar_id: "j", calendar_title: "FELIPE TORRES — GMAIL" },
      ],
      [
        { id: "g", account: "Google" },
        { id: "s", account: "felipe@skyward.ai" },
        { id: "j", account: "felipe@jsconf.cl" },
      ],
    );
    render(<OverlayAlert />);
    expect(await screen.findByText("Calendar origins")).toBeInTheDocument(); // plural
    expect(screen.getByText("Google")).toBeInTheDocument();
    expect(screen.getByText("felipe@skyward.ai")).toBeInTheDocument();
    expect(screen.getByText("felipe@jsconf.cl")).toBeInTheDocument();
  });

  it("collapses colleague-calendar duplicates that live under ONE account to a single origin", async () => {
    // The user's own calendar + a subscribed colleague's calendar (titled by the
    // colleague's email) — both under felipe@skyward.ai. Account-level dedup must
    // show ONE origin, not surface the colleague's email as if it were a second
    // place (that was the bug: it read like an invitee list).
    mockBackend(
      [
        { calendar_id: "me", calendar_title: "felipe@skyward.ai" },
        { calendar_id: "israel", calendar_title: "israel@skyward.ai" },
      ],
      [
        { id: "me", account: "felipe@skyward.ai" },
        { id: "israel", account: "felipe@skyward.ai" },
      ],
    );
    render(<OverlayAlert />);
    expect(await screen.findByText("Calendar origin")).toBeInTheDocument(); // singular
    expect(screen.getByText("felipe@skyward.ai")).toBeInTheDocument();
    // The colleague's calendar email must NOT appear as a separate origin.
    expect(screen.queryByText("israel@skyward.ai")).not.toBeInTheDocument();
  });

  it("falls back to the calendar title when accounts can't resolve, never blocking the alert", async () => {
    // list_calendars failing must not block the alert — accounts can't resolve, so
    // we fall back to the calendar title rather than dropping the row.
    invokeMock.mockImplementation((cmd: string) => {
      switch (cmd) {
        case "get_active_alarms":
          return Promise.resolve([ALARM_A]);
        case "fetch_events":
          return Promise.resolve([eventOn([{ calendar_id: "x", calendar_title: "X" }])]);
        case "list_calendars":
          return Promise.reject(new Error("blip"));
        case "get_settings":
          return Promise.resolve({ theme: "frost-dark", default_snooze_minutes: 20 });
        default:
          return Promise.resolve(undefined);
      }
    });
    render(<OverlayAlert />);
    await screen.findByText("Standup");
    // With no account resolved, fall back to the calendar title (still one origin),
    // and the alert itself is unaffected.
    expect(screen.getByText("Calendar origin")).toBeInTheDocument();
    expect(screen.getByText("X")).toBeInTheDocument();
  });
});

import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

// Event bus mock (mirrors OverlayAlert.test.tsx): capture listeners so a test can
// emit `access-state-changed`.
const listeners = vi.hoisted(() => new Map<string, (e: { payload: unknown }) => void>());
const listenMock = vi.hoisted(() => (name: string, cb: (e: { payload: unknown }) => void) => {
  listeners.set(name, cb);
  return Promise.resolve(() => listeners.delete(name));
});
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { SettingsWindow } from "./SettingsWindow";
import type { Settings } from "./registry";

const DEFAULTS: Settings = {
  enabled_calendar_ids: null,
  reminders: [5],
  alert_sound: "Sosumi",
  sound_repeat_secs: 4,
  default_snooze_minutes: 5,
  alert_tentative: true,
  alert_pending: true,
  only_video_events: false,
  show_all_day_in_tray: true,
  auto_close_enabled: false,
  auto_close_minutes: 15,
  launch_at_login: true,
  show_next_event_in_menu_bar: true,
  menu_bar_title_chars: 20,
  theme: "frost-dark",
  tray_icon: "auto",
  onboarded: true,
  telemetry_enabled: true,
  device_id: "test-device-id",
};

const CALENDARS = [
  { id: "work", title: "felipe@skyward.ai", account: "felipe@skyward.ai", color: [0.2, 0.4, 1, 1] },
  {
    id: "holidays",
    title: "Holidays in Chile",
    account: "felipe@jsconf.cl",
    color: [0, 0.8, 0.2, 1],
  },
];

beforeEach(() => {
  listeners.clear();
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "get_settings") {
      return Promise.resolve({ ...DEFAULTS });
    }
    if (cmd === "calendar_authorization_status") {
      return Promise.resolve("FullAccess");
    }
    if (cmd === "list_calendars") {
      return Promise.resolve(CALENDARS);
    }
    if (cmd === "list_system_sounds") {
      return Promise.resolve(["Basso", "Sosumi", "Submarine"]);
    }
    return Promise.resolve(undefined);
  });
});

async function renderSettings() {
  render(<SettingsWindow />);
  await waitFor(() => expect(screen.getByLabelText("Search settings")).toBeInTheDocument());
}

describe("SettingsWindow", () => {
  it("renders sidebar with all seven sections", async () => {
    await renderSettings();
    for (const label of [
      "General",
      "Alerts",
      "Calendars",
      "Event Filters",
      "Menu Bar",
      "Appearance",
      "Advanced",
    ]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
  });

  it("fuzzy search 'snooze' ranks the snooze setting first and highlights the match", async () => {
    await renderSettings();
    fireEvent.change(screen.getByLabelText("Search settings"), { target: { value: "snooze" } });
    expect(await screen.findByText(/results? for/)).toBeInTheDocument();
    const rows = document.querySelectorAll("[data-setting-id]");
    expect(rows[0].getAttribute("data-setting-id")).toBe("alerts.default-snooze");
    // <mark> highlight on the top label
    const mark = rows[0].querySelector("mark");
    expect(mark?.textContent?.toLowerCase()).toBe("snooze");
    // Settings outside the match set are filtered out
    expect(screen.queryByText("Start at login")).not.toBeInTheDocument();
  });

  it("clicking a section shows only its settings", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Event Filters" }));
    expect(screen.getByText("Alert for tentative events")).toBeInTheDocument();
    expect(screen.queryByText("Start at login")).not.toBeInTheDocument();
  });

  it("toggling a setting persists the FULL settings object via set_settings", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Event Filters" }));
    fireEvent.click(screen.getByRole("switch", { name: "Only events with a video link" }));
    await waitFor(() => {
      const call = invokeMock.mock.calls.find((call: unknown[]) => call[0] === "set_settings");
      expect(call).toBeTruthy();
      expect(call![1].settings.only_video_events).toBe(true);
      expect(call![1].settings.reminders).toEqual([5]); // rest untouched
    });
  });

  it("sends feedback via submit_feedback with the message and optional email", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Feedback" }));
    fireEvent.change(screen.getByLabelText("Your suggestion"), {
      target: { value: "add a dark theme" },
    });
    fireEvent.change(screen.getByLabelText("Your email (optional)"), {
      target: { value: "me@x.io" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      const call = invokeMock.mock.calls.find((call: unknown[]) => call[0] === "submit_feedback");
      expect(call).toBeTruthy();
      expect(call![1].message).toBe("add a dark theme");
      expect(call![1].email).toBe("me@x.io");
    });
    // Confirmation shown and the textarea is cleared for the next note.
    expect(await screen.findByText(/Thanks/)).toBeInTheDocument();
    expect(screen.getByLabelText("Your suggestion")).toHaveValue("");
  });

  it("shows the calendar-access-lost banner (pulled on mount) and Grant re-requests access", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "get_settings") {
        return Promise.resolve({ ...DEFAULTS });
      }
      if (cmd === "calendar_authorization_status") {
        return Promise.resolve("NotDetermined");
      }
      if (cmd === "list_calendars") {
        return Promise.resolve([]);
      }
      if (cmd === "list_system_sounds") {
        return Promise.resolve(["Sosumi"]);
      }
      if (cmd === "get_access_state") {
        return Promise.resolve({ state: "lost" });
      }
      return Promise.resolve(undefined);
    });
    await renderSettings();
    expect(await screen.findByRole("alert")).toHaveTextContent(/alerts are paused/i);
    fireEvent.click(screen.getByRole("button", { name: "Grant calendar access" }));
    await waitFor(() =>
      expect(invokeMock.mock.calls.some((c: unknown[]) => c[0] === "request_calendar_access")).toBe(
        true,
      ),
    );
    // A live recovery edge clears the banner.
    listeners.get("access-state-changed")?.({ payload: { state: "ok" } });
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("raises the banner on a live access-state-changed:lost edge", async () => {
    await renderSettings(); // default get_access_state → undefined → healthy
    expect(screen.queryByRole("alert")).toBeNull();
    listeners.get("access-state-changed")?.({ payload: { state: "lost" } });
    expect(await screen.findByRole("alert")).toHaveTextContent(/alerts are paused/i);
  });

  it("clears a stale error banner once a later save succeeds", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Event Filters" }));
    const toggle = () =>
      fireEvent.click(screen.getByRole("switch", { name: "Only events with a video link" }));

    // First save fails → error banner appears.
    invokeMock.mockImplementationOnce((cmd: string) =>
      cmd === "set_settings" ? Promise.reject("disk full") : Promise.resolve(undefined),
    );
    toggle();
    expect(await screen.findByText("disk full")).toBeInTheDocument();

    // A subsequent successful save must clear it (was sticky before).
    toggle();
    await waitFor(() => expect(screen.queryByText("disk full")).not.toBeInTheDocument());
  });

  it("calendar list groups by account; unchecking one stores the remaining ids", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Calendars" }));
    expect(await screen.findByText("felipe@jsconf.cl")).toBeInTheDocument(); // account header
    fireEvent.click(screen.getByRole("checkbox", { name: /Holidays in Chile/ }));
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((call: unknown[]) => call[0] === "set_settings");
      expect(call![1].settings.enabled_calendar_ids).toEqual(["work"]);
    });
  });

  it("re-checking every calendar stores null (all, future-proof)", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "get_settings") {
        return Promise.resolve({ ...DEFAULTS, enabled_calendar_ids: ["work"] });
      }
      if (cmd === "calendar_authorization_status") {
        return Promise.resolve("FullAccess");
      }
      if (cmd === "list_calendars") {
        return Promise.resolve(CALENDARS);
      }
      if (cmd === "list_system_sounds") {
        return Promise.resolve([]);
      }
      return Promise.resolve(undefined);
    });
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Calendars" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: /Holidays in Chile/ }));
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((call: unknown[]) => call[0] === "set_settings");
      expect(call![1].settings.enabled_calendar_ids).toBeNull();
    });
  });

  it("shows a Grant button when calendar access is undetermined and requests on click", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "get_settings") {
        return Promise.resolve({ ...DEFAULTS });
      }
      if (cmd === "calendar_authorization_status") {
        return Promise.resolve("NotDetermined");
      }
      if (cmd === "list_calendars") {
        return Promise.reject(new Error("not authorized"));
      }
      return Promise.resolve(undefined);
    });
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Calendars" }));
    const grant = await screen.findByRole("button", { name: "Grant calendar access" });
    fireEvent.click(grant);
    await waitFor(() => {
      expect(invokeMock.mock.calls.some((c: unknown[]) => c[0] === "request_calendar_access")).toBe(
        true,
      );
    });
  });

  it("About tab shows Check for Updates and opens the issue tracker via open_url", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "About" }));
    expect(await screen.findByRole("button", { name: "Check for Updates" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Report an issue" }));
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((c: unknown[]) => c[0] === "open_url");
      expect(call).toBeDefined();
      expect((call![1] as { url: string }).url).toContain("/issues/new");
    });
  });

  it("About tab lists the release history and expands a version's notes", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "About" }));
    // Newest backfilled version is present as a collapsible header.
    const header = await screen.findByRole("button", { name: /v0\.9\.0/ });
    expect(header).toHaveAttribute("aria-expanded", "false");
    // A body-only phrase (not the title) is hidden until expanded, then revealed.
    expect(screen.queryByText(/double takeovers/i)).not.toBeInTheDocument();
    fireEvent.click(header);
    expect(header).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText(/double takeovers/i)).toBeInTheDocument();
  });

  it("editing a reminder clamps to range and persists", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Alerts" }));
    const input = screen.getByLabelText("Reminder 1");
    fireEvent.change(input, { target: { value: "999" } });
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((call: unknown[]) => call[0] === "set_settings");
      expect(call![1].settings.reminders).toEqual([120]); // clamped to max
    });
  });

  it("adds a second and third reminder, then hides Add at the three-reminder max", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Alerts" }));
    // Start with one reminder (the default). Add two more → three inputs.
    fireEvent.click(screen.getByRole("button", { name: "Add reminder" }));
    await waitFor(() => expect(screen.getByLabelText("Reminder 2")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Add reminder" }));
    await waitFor(() => expect(screen.getByLabelText("Reminder 3")).toBeInTheDocument());
    // At three reminders the Add button is gone (max of three pre-event reminders).
    expect(screen.queryByRole("button", { name: "Add reminder" })).toBeNull();
    const call = invokeMock.mock.calls.findLast((c: unknown[]) => c[0] === "set_settings");
    expect(call![1].settings.reminders).toHaveLength(3);
  });

  it("removing every reminder persists an empty list (only the start alert remains)", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Alerts" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove reminder 1" }));
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((c: unknown[]) => c[0] === "set_settings");
      expect(call![1].settings.reminders).toEqual([]);
    });
    // With zero reminders there is no reminder input, but the schedule is still valid.
    expect(screen.queryByLabelText("Reminder 1")).toBeNull();
  });

  it("changing the default snooze duration persists independently of the reminders", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Alerts" }));
    fireEvent.change(screen.getByLabelText("Default snooze duration"), { target: { value: "20" } });
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((c: unknown[]) => c[0] === "set_settings");
      expect(call![1].settings.default_snooze_minutes).toBe(20);
      expect(call![1].settings.reminders).toEqual([5]); // reminder schedule untouched
    });
  });

  it("sound picker previews on change", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Alerts" }));
    fireEvent.change(screen.getByLabelText("Alert sound"), { target: { value: "Submarine" } });
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("preview_sound", { name: "Submarine" });
    });
  });

  it("appearance: theme picker persists and demo button fires demo_alert", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    fireEvent.change(screen.getByLabelText("Alert theme"), { target: { value: "sunset" } });
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((c: unknown[]) => c[0] === "set_settings");
      expect(call![1].settings.theme).toBe("sunset");
    });
    fireEvent.click(screen.getByRole("button", { name: "Show Demo Alert" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("demo_alert");
    });
  });

  it("menu bar: next-event toggle persists", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Menu Bar" }));
    fireEvent.click(screen.getByRole("switch", { name: "Show next event in the menu bar" }));
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((c: unknown[]) => c[0] === "set_settings");
      expect(call![1].settings.show_next_event_in_menu_bar).toBe(false);
    });
  });

  it("garbage search shows the empty state", async () => {
    await renderSettings();
    fireEvent.change(screen.getByLabelText("Search settings"), { target: { value: "qqqqxxxx" } });
    expect(await screen.findByText("No settings match.")).toBeInTheDocument();
  });

  // Regression: the "stopped responding" loss mode (poisoned TCC record) needs a
  // different CTA than a plain revocation — "Repair access" fires repair_calendar_access,
  // NOT request_calendar_access. Conflating them would send the user through a
  // redundant re-prompt that can't actually fix the corrupted record.
  it("get_access_state:lost+fetch_failed_despite_authorized → Repair banner; clicking invokes repair_calendar_access", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "get_settings") {
        return Promise.resolve({ ...DEFAULTS });
      }
      if (cmd === "calendar_authorization_status") {
        return Promise.resolve("FullAccess");
      }
      if (cmd === "list_calendars") {
        return Promise.resolve(CALENDARS);
      }
      if (cmd === "list_system_sounds") {
        return Promise.resolve(["Sosumi"]);
      }
      if (cmd === "get_access_state") {
        return Promise.resolve({ state: "lost", reason: "fetch_failed_despite_authorized" });
      }
      return Promise.resolve(undefined);
    });
    await renderSettings();
    const banner = await screen.findByRole("alert");
    // Scoped to the access banner: must NOT fire for an unrelated alert element.
    expect(banner).toHaveTextContent(/Calendar stopped responding — alerts are paused/i);
    // The "Grant" button must not appear for this loss mode.
    expect(screen.queryByRole("button", { name: "Grant calendar access" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Repair access" }));
    await waitFor(() =>
      expect(invokeMock.mock.calls.some((c: unknown[]) => c[0] === "repair_calendar_access")).toBe(
        true,
      ),
    );
    // Confirm the wrong command was never called.
    expect(invokeMock.mock.calls.some((c: unknown[]) => c[0] === "request_calendar_access")).toBe(
      false,
    );
  });

  // Regression: plain access revocation (authorization_not_determined, denied, etc.)
  // must route to the Grant CTA, not the Repair CTA — the two loss modes are distinct.
  it("get_access_state:lost+authorization_not_determined → Grant banner; clicking invokes request_calendar_access", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "get_settings") {
        return Promise.resolve({ ...DEFAULTS });
      }
      if (cmd === "calendar_authorization_status") {
        return Promise.resolve("FullAccess");
      }
      if (cmd === "list_calendars") {
        return Promise.resolve(CALENDARS);
      }
      if (cmd === "list_system_sounds") {
        return Promise.resolve(["Sosumi"]);
      }
      if (cmd === "get_access_state") {
        return Promise.resolve({ state: "lost", reason: "authorization_not_determined" });
      }
      return Promise.resolve(undefined);
    });
    await renderSettings();
    const banner = await screen.findByRole("alert");
    expect(banner).toHaveTextContent(/Calendar access was lost — alerts are paused/i);
    // The "Repair" button must not appear for a plain revocation.
    expect(screen.queryByRole("button", { name: "Repair access" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Grant calendar access" }));
    await waitFor(() =>
      expect(invokeMock.mock.calls.some((c: unknown[]) => c[0] === "request_calendar_access")).toBe(
        true,
      ),
    );
  });

  // Regression: the banner must react to live events — not just the mount-time pull.
  // An initial healthy state followed by a fetch_failed_despite_authorized edge must
  // raise the Repair banner; a subsequent ok edge must clear it without a reload.
  it("live access-state-changed events: lost→Repair banner, then ok→banner clears", async () => {
    // Default invokeMock: get_access_state returns undefined → healthy on mount.
    await renderSettings();
    expect(screen.queryByRole("alert")).toBeNull();

    // Emit the "stopped responding" loss edge.
    listeners.get("access-state-changed")?.({
      payload: { state: "lost", reason: "fetch_failed_despite_authorized" },
    });
    const banner = await screen.findByRole("alert");
    expect(banner).toHaveTextContent(/Calendar stopped responding — alerts are paused/i);
    expect(screen.getByRole("button", { name: "Repair access" })).toBeInTheDocument();

    // Recovery edge must clear the banner.
    listeners.get("access-state-changed")?.({ payload: { state: "ok" } });
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  // Regression: a payload with no reason field (e.g. legacy Rust emission or partial
  // payloads during startup) must not crash and must fall through to the Grant branch —
  // the safer, user-actionable default.
  it("access-state-changed with reason:undefined falls back to Grant branch without crashing", async () => {
    await renderSettings();
    // Fire a lost event with no reason property at all.
    listeners.get("access-state-changed")?.({ payload: { state: "lost" } });
    const banner = await screen.findByRole("alert");
    expect(banner).toHaveTextContent(/Calendar access was lost — alerts are paused/i);
    expect(screen.getByRole("button", { name: "Grant calendar access" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Repair access" })).toBeNull();
  });
});

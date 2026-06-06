import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { SettingsWindow } from "./SettingsWindow";
import type { Settings } from "./registry";

const DEFAULTS: Settings = {
  enabled_calendar_ids: null,
  lead_minutes: 5,
  alert_sound: "Sosumi",
  sound_repeat_secs: 4,
  snooze_minutes: [1, 5],
  alert_tentative: true,
  alert_pending: true,
  only_video_events: false,
  show_all_day_in_tray: true,
  auto_close_enabled: false,
  auto_close_minutes: 15,
  launch_at_login: true,
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
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "get_settings") {
      return Promise.resolve({ ...DEFAULTS });
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
    expect(rows[0].getAttribute("data-setting-id")).toBe("alerts.snooze-durations");
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
      expect(call![1].settings.lead_minutes).toBe(5); // rest untouched
    });
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

  it("changing lead minutes clamps to range and persists", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Alerts" }));
    const input = screen.getByLabelText("Alert me before the event");
    fireEvent.change(input, { target: { value: "999" } });
    await waitFor(() => {
      const call = invokeMock.mock.calls.findLast((call: unknown[]) => call[0] === "set_settings");
      expect(call![1].settings.lead_minutes).toBe(60); // clamped to max
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

  it("placeholder settings render their note instead of a control", async () => {
    await renderSettings();
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    expect(screen.getByText(/Coming soon — frosted glass/)).toBeInTheDocument();
  });

  it("garbage search shows the empty state", async () => {
    await renderSettings();
    fireEvent.change(screen.getByLabelText("Search settings"), { target: { value: "qqqqxxxx" } });
    expect(await screen.findByText("No settings match.")).toBeInTheDocument();
  });
});

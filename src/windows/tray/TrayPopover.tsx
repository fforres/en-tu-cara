// Tray popover (Phase 4) — styling deliberately RAW macOS: system-ui font,
// CSS system colors, color-scheme aware. No custom chrome.
//
// DORMANT: the tray icon now opens a native menu (Open Settings · Quit) instead
// of this popover — see src-tauri/src/tray.rs. This component is no longer
// rendered by App.tsx (kept, with its test, for possible reuse). Do NOT wire it
// to the hidden `background` window: that window must render nothing.

import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  elapsedFraction,
  groupUpcomingByDay,
  ongoingSorted,
  remainingLabel,
} from "../../lib/classify";
import { extractMeetingLink, type MeetingLink } from "../../lib/meeting-links";

export interface UiEvent {
  occurrence_key: string;
  title: string;
  start: string;
  end: string;
  all_day: boolean;
  status: string;
  my_rsvp: string | null;
  is_recurring_occurrence: boolean;
  calendar_title: string | null;
  calendar_id: string | null;
  url: string | null;
  location: string | null;
  notes: string | null;
}

interface CalendarInfo {
  id: string;
  title: string;
  account: string | null;
  color: [number, number, number, number] | null;
}

const css = {
  font: "13px system-ui, -apple-system, sans-serif",
  hairline: "1px solid color-mix(in srgb, CanvasText 12%, transparent)",
  secondary: "color-mix(in srgb, CanvasText 55%, transparent)",
} as const;

function cssColor(c: CalendarInfo["color"]): string {
  if (!c) {
    return "GrayText";
  }
  const [r, g, b, a] = c;
  return `rgba(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)}, ${a})`;
}

function timeRange(e: UiEvent): string {
  const opts: Intl.DateTimeFormatOptions = { hour: "numeric", minute: "2-digit" };
  const start = new Date(e.start);
  const day = start.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
  return `${day} at ${start.toLocaleTimeString(undefined, opts)} – ${new Date(e.end).toLocaleTimeString(undefined, opts)}`;
}

function Pie({ fraction }: { fraction: number }) {
  const deg = Math.round(fraction * 360);
  return (
    <span
      aria-label="time elapsed"
      style={{
        width: 14,
        height: 14,
        borderRadius: "50%",
        display: "inline-block",
        background: `conic-gradient(GrayText ${deg}deg, color-mix(in srgb, GrayText 25%, transparent) ${deg}deg)`,
      }}
    />
  );
}

function CameraIcon({ link, dimmed }: { link: MeetingLink | null; dimmed?: boolean }) {
  return (
    <button
      title={link ? `Join ${link.provider}` : "No video link"}
      disabled={!link}
      onClick={() => link && openUrl(link.url)}
      style={{
        border: "none",
        background: "none",
        cursor: link ? "pointer" : "default",
        opacity: link ? 1 : 0.25,
        fontSize: 14,
        padding: 2,
        filter: dimmed ? "grayscale(1)" : undefined,
      }}
    >
      📹
    </button>
  );
}

function EventRow({ event, now, ongoing }: { event: UiEvent; now: Date; ongoing?: boolean }) {
  const link = useMemo(() => extractMeetingLink(event), [event]);
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "5px 12px",
        borderLeft: `3px solid transparent`,
      }}
    >
      <span
        style={{
          width: 3,
          alignSelf: "stretch",
          borderRadius: 2,
          background: "var(--cal-color, GrayText)",
        }}
      />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            fontWeight: ongoing ? 600 : 500,
          }}
        >
          {event.is_recurring_occurrence ? "↻ " : ""}
          {event.title}
        </div>
        <div style={{ color: css.secondary, fontSize: 11 }}>
          {ongoing
            ? `until ${new Date(event.end).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}`
            : timeRange(event)}
        </div>
        {link && !ongoing && (
          <div
            style={{
              color: css.secondary,
              fontSize: 11,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {link.url}
          </div>
        )}
      </div>
      {ongoing && (
        <span
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            fontSize: 11,
            color: css.secondary,
          }}
        >
          <Pie fraction={elapsedFraction(event, now)} />
          {remainingLabel(event, now)}
        </span>
      )}
      <CameraIcon link={link} dimmed={!ongoing} />
    </div>
  );
}

export function TrayPopover() {
  const [events, setEvents] = useState<UiEvent[]>([]);
  const [calendars, setCalendars] = useState<Map<string, CalendarInfo>>(new Map());
  const [now, setNow] = useState(() => new Date());
  const [todayOnly, setTodayOnly] = useState(false);
  const [paused, setPaused] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [evs, cals, isPaused] = await Promise.all([
        invoke<UiEvent[]>("fetch_events", { daysBack: 1, daysForward: 7 }),
        invoke<CalendarInfo[]>("list_calendars"),
        invoke<boolean>("get_paused"),
      ]);
      setEvents(evs);
      setCalendars(new Map(cals.map((c) => [c.id, c])));
      setPaused(isPaused);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const data = setInterval(() => void refresh(), 30_000); // poll backstop (PLAN §1)
    const clock = setInterval(() => setNow(new Date()), 15_000);
    const onFocus = () => void refresh(); // popover shown → instant freshness
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(data);
      clearInterval(clock);
      window.removeEventListener("focus", onFocus);
    };
  }, [refresh]);

  const ongoing = ongoingSorted(events, now);
  const groups = groupUpcomingByDay(events, now, todayOnly);

  const sectionHeader = (label: string, right?: React.ReactNode) => (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        padding: "8px 12px 4px",
        fontSize: 11,
        fontWeight: 700,
        letterSpacing: 0.4,
        color: css.secondary,
        textTransform: "uppercase",
      }}
    >
      {label}
      {right}
    </div>
  );

  return (
    <main
      style={{
        font: css.font,
        colorScheme: "light dark",
        background: "Canvas",
        color: "CanvasText",
        height: "100vh",
        display: "flex",
        flexDirection: "column",
        userSelect: "none",
        overflow: "hidden",
      }}
    >
      {/* Header bar */}
      <header
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: "8px 12px",
          borderBottom: css.hairline,
        }}
      >
        <strong style={{ fontSize: 13 }}>En Tu Cara</strong>
        <span style={{ display: "flex", gap: 10 }}>
          <button
            title={paused ? "Resume alerts" : "Pause alerts"}
            onClick={async () => {
              await invoke("set_paused", { paused: !paused });
              setPaused(!paused);
            }}
            style={{ border: "none", background: "none", cursor: "pointer", fontSize: 14 }}
          >
            {paused ? "▶️" : "⏸️"}
          </button>
          <button
            title="Settings"
            onClick={() => void invoke("open_settings")}
            style={{ border: "none", background: "none", cursor: "pointer", fontSize: 14 }}
          >
            ⚙️
          </button>
        </span>
      </header>

      {paused && (
        <div
          style={{
            padding: "4px 12px",
            fontSize: 11,
            background: "color-mix(in srgb, orange 18%, Canvas)",
            color: "CanvasText",
          }}
        >
          Alerts are paused
        </div>
      )}
      {error && <div style={{ padding: "6px 12px", fontSize: 11, color: "crimson" }}>{error}</div>}

      <div style={{ flex: 1, overflowY: "auto" }}>
        {ongoing.length > 0 && (
          <>
            {sectionHeader("Ongoing events")}
            {ongoing.map((e) => (
              <div
                key={e.occurrence_key}
                style={{
                  ["--cal-color" as string]: cssColor(
                    calendars.get(e.calendar_id ?? "")?.color ?? null,
                  ),
                }}
              >
                <EventRow event={e} now={now} ongoing />
              </div>
            ))}
          </>
        )}

        {sectionHeader(
          "Upcoming events",
          <span
            style={{
              display: "inline-flex",
              border: css.hairline,
              borderRadius: 5,
              overflow: "hidden",
              textTransform: "none",
              fontWeight: 400,
            }}
          >
            {(["today", "all"] as const).map((mode) => (
              <button
                key={mode}
                onClick={() => setTodayOnly(mode === "today")}
                style={{
                  border: "none",
                  fontSize: 11,
                  padding: "2px 8px",
                  cursor: "pointer",
                  background: (mode === "today") === todayOnly ? "Highlight" : "transparent",
                  color: (mode === "today") === todayOnly ? "HighlightText" : "CanvasText",
                }}
              >
                {mode}
              </button>
            ))}
          </span>,
        )}
        {groups.length === 0 && (
          <div style={{ padding: "16px 12px", color: css.secondary, fontSize: 12 }}>
            No upcoming events {todayOnly ? "today" : "in the next 7 days"}.
          </div>
        )}
        {groups.map((g) => (
          <section key={g.dateKey}>
            <div style={{ padding: "6px 12px 2px", fontWeight: 600, fontSize: 12 }}>{g.label}</div>
            {g.events.map((e) => (
              <div
                key={e.occurrence_key}
                style={{
                  ["--cal-color" as string]: cssColor(
                    calendars.get(e.calendar_id ?? "")?.color ?? null,
                  ),
                }}
              >
                <EventRow event={e} now={now} />
              </div>
            ))}
          </section>
        ))}
      </div>
    </main>
  );
}

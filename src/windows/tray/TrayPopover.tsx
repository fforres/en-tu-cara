// Tray popover (Phase 4) — styling deliberately RAW macOS: system-ui font,
// CSS system colors, color-scheme aware. No custom chrome.
//
// Rendered in the `popover` NSPanel window (App.tsx), shown under the tray icon
// on left-click (src-tauri/src/tray.rs). Right-click the tray for the menu.

import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  elapsedFraction,
  groupUpcomingByDay,
  ongoingSorted,
  remainingLabel,
} from "../../lib/classify";
import { extractMeetingLink } from "../../lib/meeting-links";

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

const menuItemStyle: React.CSSProperties = {
  display: "block",
  width: "100%",
  textAlign: "left",
  font: "inherit",
  fontSize: 12,
  padding: "6px 10px",
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: "CanvasText",
  cursor: "pointer",
  whiteSpace: "nowrap",
};

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

// Calendar origin: the account (email) the calendar lives under, then the
// sub-calendar's name — e.g. "felipe@skyward.ai · Team Events". Deduped + empties
// dropped so a single-name account doesn't read "Work · Work".
function calendarOrigin(event: UiEvent, calendar?: CalendarInfo): string {
  return [calendar?.account, calendar?.title ?? event.calendar_title]
    .filter((s): s is string => Boolean(s))
    .filter((s, i, all) => all.indexOf(s) === i)
    .join(" · ");
}

function EventRow({
  event,
  now,
  ongoing,
  ignored,
  calendar,
  onContextMenu,
}: {
  event: UiEvent;
  now: Date;
  ongoing?: boolean;
  ignored: boolean;
  calendar?: CalendarInfo;
  onContextMenu: (x: number, y: number, occurrenceKey: string, link: string | null) => void;
}) {
  const link = useMemo(() => extractMeetingLink(event), [event]);
  const open = () => {
    if (link) {
      void openUrl(link.url).catch(() => {});
    }
  };
  const origin = calendarOrigin(event, calendar);
  return (
    <div
      onClick={open}
      onContextMenu={(e) => {
        e.preventDefault();
        onContextMenu(e.clientX, e.clientY, event.occurrence_key, link?.url ?? null);
      }}
      title={
        ignored
          ? "Ignored — right-click to stop ignoring"
          : link
            ? "Open meeting · right-click for options"
            : "Right-click for options"
      }
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 8,
        padding: "6px 12px",
        borderLeft: `3px solid transparent`,
        cursor: link ? "pointer" : "context-menu",
        opacity: ignored ? 0.45 : 1,
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
            textDecoration: ignored ? "line-through" : undefined,
          }}
        >
          {event.is_recurring_occurrence ? "↻ " : ""}
          {event.title}
          {ignored && (
            <span
              style={{
                marginLeft: 6,
                fontSize: 10,
                fontWeight: 600,
                color: css.secondary,
                textDecoration: "none",
              }}
            >
              IGNORED
            </span>
          )}
        </div>

        {/* Meta row: date + calendar origin on the left, "Go to event" on the right. */}
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 2 }}>
          <div style={{ flex: 1, minWidth: 0, color: css.secondary, fontSize: 11 }}>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 4,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {ongoing ? (
                <>
                  until{" "}
                  {new Date(event.end).toLocaleTimeString(undefined, {
                    hour: "numeric",
                    minute: "2-digit",
                  })}
                  <Pie fraction={elapsedFraction(event, now)} />
                  {remainingLabel(event, now)}
                </>
              ) : (
                timeRange(event)
              )}
            </div>
            {origin && (
              <div style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {origin}
              </div>
            )}
          </div>
          {link && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                open();
              }}
              style={{
                flexShrink: 0,
                font: "inherit",
                fontSize: 11,
                fontWeight: 600,
                padding: "3px 12px",
                borderRadius: 5,
                border: css.hairline,
                background: "color-mix(in srgb, AccentColor 16%, transparent)",
                color: "CanvasText",
                cursor: "pointer",
              }}
            >
              Go to event
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

export function TrayPopover() {
  const [events, setEvents] = useState<UiEvent[]>([]);
  const [calendars, setCalendars] = useState<Map<string, CalendarInfo>>(new Map());
  const [now, setNow] = useState(() => new Date());
  const [todayOnly, setTodayOnly] = useState(false);
  const [paused, setPaused] = useState(false);
  const [ignored, setIgnored] = useState<Set<string>>(new Set());
  // Right-click context menu: which occurrence + where to render it.
  const [menu, setMenu] = useState<{
    key: string;
    x: number;
    y: number;
    link: string | null;
  } | null>(null);

  const refresh = useCallback(async () => {
    // Each read degrades on its own — never surface a backend failure in the
    // popover UI. Most commonly fetch_events fails because the process has no
    // calendar access (a bare dev binary), which should just look like "no
    // events", not an error banner.
    const [evs, cals, isPaused, ign] = await Promise.all([
      invoke<UiEvent[]>("fetch_events", { daysBack: 1, daysForward: 7 }).catch(
        () => [] as UiEvent[],
      ),
      invoke<CalendarInfo[]>("list_calendars").catch(() => [] as CalendarInfo[]),
      invoke<boolean>("get_paused").catch(() => false),
      invoke<string[]>("get_ignored").catch(() => [] as string[]),
    ]);
    setEvents(evs);
    setCalendars(new Map(cals.map((c) => [c.id, c])));
    setPaused(isPaused);
    setIgnored(new Set(ign));
  }, []);

  const openMenu = useCallback(
    (x: number, y: number, key: string, link: string | null) => setMenu({ key, x, y, link }),
    [],
  );

  const toggleIgnore = useCallback(
    (key: string) => {
      const isIgnored = ignored.has(key);
      const cmd = isIgnored ? "unignore_occurrence" : "ignore_occurrence";
      // Optimistic: flip locally now, persist in the backend (outside the state
      // updater so StrictMode can't double-fire the command).
      setIgnored((prev) => {
        const next = new Set(prev);
        if (isIgnored) {
          next.delete(key);
        } else {
          next.add(key);
        }
        return next;
      });
      void invoke(cmd, { occurrenceKey: key }).catch((e) => console.warn("toggle ignore:", e));
      setMenu(null);
    },
    [ignored],
  );

  // Dismiss the context menu on Escape or when the popover loses focus (it hides
  // on blur, so a stale menu must not linger into the next open).
  useEffect(() => {
    const close = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        close();
      }
    };
    if (menu) {
      window.addEventListener("keydown", onKey);
      window.addEventListener("blur", close);
    }
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  useEffect(() => {
    // The popover is the always-loaded host window; it's hidden ~99% of the
    // time. Only poll EventKit + tick the countdown clock while it's actually on
    // screen — otherwise a menu-bar app idle in the tray burns battery doing 3
    // IPC round-trips every 30s and a full re-render every 15s for no viewer.
    let data: ReturnType<typeof setInterval> | null = null;
    let clock: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      void refresh(); // shown → instant freshness
      setNow(new Date());
      data ??= setInterval(() => void refresh(), 30_000); // poll backstop
      clock ??= setInterval(() => setNow(new Date()), 15_000);
    };
    const stop = () => {
      if (data !== null) {
        clearInterval(data);
        data = null;
      }
      if (clock !== null) {
        clearInterval(clock);
        clock = null;
      }
    };
    // Focus/blur is the reliable signal for a dismiss-on-blur popover.
    const sync = () =>
      document.visibilityState === "visible" || document.hasFocus() ? start() : stop();
    sync();
    window.addEventListener("focus", start);
    window.addEventListener("blur", stop);
    document.addEventListener("visibilitychange", sync);
    return () => {
      stop();
      window.removeEventListener("focus", start);
      window.removeEventListener("blur", stop);
      document.removeEventListener("visibilitychange", sync);
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
              const next = !paused;
              try {
                await invoke("set_paused", { paused: next });
                setPaused(next); // state follows the backend, not the other way
              } catch (e) {
                console.warn("set_paused:", e);
              }
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
                <EventRow
                  event={e}
                  now={now}
                  ongoing
                  ignored={ignored.has(e.occurrence_key)}
                  calendar={calendars.get(e.calendar_id ?? "")}
                  onContextMenu={openMenu}
                />
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
                <EventRow
                  event={e}
                  now={now}
                  ignored={ignored.has(e.occurrence_key)}
                  calendar={calendars.get(e.calendar_id ?? "")}
                  onContextMenu={openMenu}
                />
              </div>
            ))}
          </section>
        ))}
      </div>

      {menu && (
        <>
          {/* Click-away backdrop (mousedown so it beats any row click). */}
          <div
            onMouseDown={() => setMenu(null)}
            style={{ position: "fixed", inset: 0, zIndex: 50 }}
          />
          <div
            role="menu"
            style={{
              position: "fixed",
              left: Math.min(menu.x, window.innerWidth - 180),
              top: Math.min(menu.y, window.innerHeight - 48),
              zIndex: 51,
              minWidth: 168,
              background: "Canvas",
              border: css.hairline,
              borderRadius: 6,
              boxShadow: "0 6px 22px rgba(0, 0, 0, 0.28)",
              padding: 4,
            }}
          >
            {menu.link && (
              <>
                <button
                  onClick={() => {
                    const url = menu.link;
                    setMenu(null);
                    if (url) {
                      void openUrl(url).catch((e) => console.warn("open in browser:", e));
                    }
                  }}
                  style={menuItemStyle}
                >
                  Open in browser
                </button>
                <button
                  onClick={() => {
                    const url = menu.link;
                    setMenu(null);
                    if (url) {
                      void navigator.clipboard?.writeText(url).catch(() => {});
                    }
                  }}
                  style={menuItemStyle}
                >
                  Copy link
                </button>
                <div
                  style={{
                    height: 1,
                    background: "color-mix(in srgb, CanvasText 12%, transparent)",
                    margin: "4px 6px",
                  }}
                />
              </>
            )}
            <button onClick={() => toggleIgnore(menu.key)} style={menuItemStyle}>
              {ignored.has(menu.key) ? "Stop ignoring this event" : "Ignore this event"}
            </button>
          </div>
        </>
      )}
    </main>
  );
}

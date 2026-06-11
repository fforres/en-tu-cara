// Tray popover (Phase 4) — styling deliberately RAW macOS: system-ui font,
// CSS system colors, color-scheme aware. No custom chrome.
//
// Rendered in the `popover` NSPanel window (App.tsx), shown under the tray icon
// on left-click (src-tauri/src/tray.rs). Right-click the tray for the menu.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  elapsedFraction,
  groupUpcomingByDay,
  ongoingSorted,
  remainingLabel,
} from "../../lib/classify";
import { extractMeetingLink, isWebUrl } from "../../lib/meeting-links";

export interface UiEvent {
  /** EKEvent identifier (event.eventIdentifier) — used for the ical:// deep-link. */
  id: string;
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
  onContextMenu: (
    x: number,
    y: number,
    occurrenceKey: string,
    eventId: string,
    link: string | null,
    endsAt: string,
  ) => void;
}) {
  const link = useMemo(() => extractMeetingLink(event), [event]);
  // The videocall/join link (button + right-click "Open in browser").
  const openVideocall = () => {
    if (link && isWebUrl(link.url)) {
      void openUrl(link.url).catch(() => {});
    }
  };
  // Row-click opens the calendar event ON THE WEB. EventKit only gives us
  // EKEvent.URL (event.url) — there is NO separate provider "view event on the
  // web" link — so the best feasible behavior is: open event.url when it's a
  // web URL, else no-op. (For many Meet/Zoom invites event.url is empty or the
  // same join link; see report.)
  const webUrl = event.url && isWebUrl(event.url) ? event.url : null;
  const openWebEvent = () => {
    if (webUrl) {
      void openUrl(webUrl).catch(() => {});
    }
  };
  const origin = calendarOrigin(event, calendar);
  return (
    <div
      onClick={openWebEvent}
      onContextMenu={(e) => {
        e.preventDefault();
        onContextMenu(
          e.clientX,
          e.clientY,
          event.occurrence_key,
          event.id,
          link?.url ?? null,
          event.end,
        );
      }}
      title={
        ignored
          ? "Ignored — right-click to stop ignoring"
          : webUrl
            ? "Open event on the web · right-click for options"
            : "Right-click for options"
      }
      style={{
        display: "flex",
        alignItems: "flex-start",
        gap: 8,
        padding: "6px 12px",
        borderLeft: `3px solid transparent`,
        cursor: webUrl ? "pointer" : "context-menu",
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
                openVideocall();
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
              Open videocall
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
  // Calendar-access health. When lost, the list keeps showing the last-known
  // events (preserve-on-failure, see refresh) but with a banner warning they may
  // be outdated — consistent with the menu-bar ⚠️ and the Settings banner.
  // `accessReason` picks the copy: a revoked grant (user must re-grant) vs reads
  // failing despite a grant (self-repairs; the events shown are a stale
  // snapshot, not gone).
  const [accessLost, setAccessLost] = useState(false);
  const [accessReason, setAccessReason] = useState("");
  const [ignored, setIgnored] = useState<Set<string>>(new Set());
  // Mirror of `ignored` so toggleIgnore decides direction from CURRENT state
  // (not a stale closure) without depending on `ignored` — a fast click can't
  // send the wrong command (bug H3b).
  const ignoredRef = useRef(ignored);
  useEffect(() => {
    ignoredRef.current = ignored;
  }, [ignored]);
  // Right-click context menu: which occurrence + where to render it.
  const [menu, setMenu] = useState<{
    key: string;
    eventId: string;
    x: number;
    y: number;
    link: string | null;
    endsAt: string;
  } | null>(null);

  const refreshingRef = useRef(false);
  const refresh = useCallback(async () => {
    // Coalesce overlapping refreshes: if one is already in flight (e.g. a 30s
    // tick lands while the on-show refresh is still awaiting EventKit), skip
    // rather than pile a second batch of fetch_events onto the command pool.
    if (refreshingRef.current) {
      return;
    }
    refreshingRef.current = true;
    try {
      // Each read degrades on its own — never surface a backend failure in the
      // popover UI. Most commonly the events read fails because the process has
      // no calendar access (a bare dev binary), which should just look like "no
      // events", not an error banner.
      //
      // `refresh_popover` is the events read: it fetches the upcoming list AND
      // refreshes the menu-bar "next event" title from that SAME list, so the
      // menu-bar text never lags behind what we show here (it's the single
      // next-event derivation, shared with the background scheduler heartbeat).
      //
      // PRESERVE-ON-FAILURE (regression fix): a transient EventKit blip makes
      // the read reject. Resolving that to `[]` and calling setEvents([]) CLEARS
      // the visible list — events "disappear and can't be clicked", and stay
      // gone if later polls also blip. So a read that fails resolves to `null`
      // and we SKIP the corresponding setState, keeping the last-good data on
      // screen. Only a SUCCESSFUL read replaces state.
      const [evs, cals, isPaused, ign] = await Promise.all([
        invoke<UiEvent[]>("refresh_popover", { daysBack: 1, daysForward: 7 }).catch((e) => {
          console.warn("refresh_popover:", e);
          return null;
        }),
        invoke<CalendarInfo[]>("list_calendars").catch((e) => {
          console.warn("list_calendars:", e);
          return null;
        }),
        invoke<boolean>("get_paused").catch((e) => {
          console.warn("get_paused:", e);
          // null, not false: a transient IPC blip must PRESERVE the last-good
          // pause toggle, never silently flip it to "running" (same preserve
          // discipline as the other three reads above).
          return null;
        }),
        invoke<string[]>("get_ignored").catch((e) => {
          console.warn("get_ignored:", e);
          return null;
        }),
      ]);
      if (evs) {
        setEvents(evs);
      }
      if (cals) {
        setCalendars(new Map(cals.map((c) => [c.id, c])));
      }
      if (isPaused !== null) {
        setPaused(isPaused);
      }
      if (ign) {
        setIgnored(new Set(ign));
      }
    } finally {
      refreshingRef.current = false;
    }
  }, []);

  const openMenu = useCallback(
    (x: number, y: number, key: string, eventId: string, link: string | null, endsAt: string) =>
      setMenu({ key, eventId, x, y, link, endsAt }),
    [],
  );

  const toggleIgnore = useCallback((key: string, endsAt: string) => {
    // Direction comes from the ref (current state), not a stale closure — a
    // fast click can't send the wrong command (H3b). invoke runs OUTSIDE the
    // setState updater so StrictMode's double-invoke of the updater can't
    // double-fire the command (H3 / mirrors SettingsWindow.update).
    const wasIgnored = ignoredRef.current.has(key);
    const cmd = wasIgnored ? "unignore_occurrence" : "ignore_occurrence";
    // Optimistic flip.
    setIgnored((prev) => {
      const next = new Set(prev);
      if (wasIgnored) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
    // ignore_occurrence needs the occurrence end so the backend GCs the ignore
    // 48h after the event ends, not 48h after the click (H1).
    const args = wasIgnored ? { occurrenceKey: key } : { occurrenceKey: key, endsAt };
    void invoke(cmd, args).catch((e) => {
      console.warn("toggle ignore:", e);
      // Revert the optimistic flip — a failed write must not leave the row
      // claiming IGNORED while the alarm still fires (H3a / mirrors toggleLogin).
      setIgnored((prev) => {
        const next = new Set(prev);
        if (wasIgnored) {
          next.add(key);
        } else {
          next.delete(key);
        }
        return next;
      });
    });
    setMenu(null);
  }, []);

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

  // Calendar-access health banner: same signal as the menu-bar ⚠️ and the
  // Settings banner (emitted by the scheduler's access machine). Pull on mount +
  // listen for live edges. The event list itself is preserved-on-failure (see
  // refresh), so when lost we still show the last-known events under the banner.
  useEffect(() => {
    invoke<{ state: string; reason?: string }>("get_access_state")
      .then((s) => {
        setAccessLost(s?.state === "lost");
        setAccessReason(s?.reason ?? "");
      })
      .catch(() => {});
    const unlisten = listen<{ state: string; reason?: string }>("access-state-changed", (e) => {
      setAccessLost(e.payload.state === "lost");
      setAccessReason(e.payload.reason ?? "");
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    // The popover is the always-loaded host window; it's hidden ~99% of the
    // time. Only poll EventKit + tick the countdown clock while it's actually on
    // screen — otherwise a menu-bar app idle in the tray burns battery for no
    // viewer.
    //
    // CRITICAL: `focus`, `visibilitychange`, AND the mount `sync()` all signal
    // "shown", so a single open used to fire refresh() ~3× (a fetch_events /
    // list_calendars STORM — confirmed in logs). Under rapid open/close that
    // floods the Tauri command pool + the React render loop and the popover
    // freezes ("events disappear, can't click"). The `active` latch makes the
    // become-visible transition IDEMPOTENT: exactly ONE refresh per show, no
    // matter how many of those events fire.
    let data: ReturnType<typeof setInterval> | null = null;
    let clock: ReturnType<typeof setInterval> | null = null;
    let active = false;
    const activate = () => {
      if (active) {
        return; // already shown — ignore duplicate focus/visibility signals
      }
      active = true;
      void refresh(); // one refresh per show
      setNow(new Date());
      // The popover panel becomes key on show (needed for click-outside
      // dismissal), and WebKit then auto-focuses the first control — the pause
      // button — so it looks "pre-selected". Clear it so the popover opens with
      // nothing highlighted. Deferred so it runs after WebKit applies the focus.
      requestAnimationFrame(() => {
        const el = document.activeElement;
        if (el instanceof HTMLElement && el !== document.body) {
          el.blur();
        }
      });
      data ??= setInterval(() => void refresh(), 30_000); // poll backstop
      clock ??= setInterval(() => setNow(new Date()), 15_000);
    };
    const deactivate = () => {
      if (!active) {
        return;
      }
      active = false;
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
      document.visibilityState === "visible" || document.hasFocus() ? activate() : deactivate();
    sync();
    window.addEventListener("focus", activate);
    window.addEventListener("blur", deactivate);
    document.addEventListener("visibilitychange", sync);
    return () => {
      deactivate();
      window.removeEventListener("focus", activate);
      window.removeEventListener("blur", deactivate);
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
            title="Send feedback or a suggestion"
            onClick={() => void invoke("open_feedback")}
            style={{ border: "none", background: "none", cursor: "pointer", fontSize: 14 }}
          >
            💬
          </button>
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

      {accessLost && (
        <div
          role="alert"
          style={{
            padding: "6px 12px",
            fontSize: 11,
            background: "color-mix(in srgb, crimson 16%, Canvas)",
            color: "CanvasText",
            borderBottom: css.hairline,
          }}
        >
          {accessReason === "fetch_failed_despite_authorized"
            ? "⚠️ Calendar stopped responding — showing last-known events (may be stale). Repairing automatically; open Settings if it persists."
            : "⚠️ Calendar access lost — these events may be outdated. Open Settings to fix."}
        </div>
      )}

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
              // Math.max(8, …) floors the position: clamping ONLY the far edge
              // let a tiny/short window (innerWidth<188, innerHeight<153) push
              // the menu off the top-left corner with a negative offset.
              left: Math.max(8, Math.min(menu.x, window.innerWidth - 180)),
              // Clamp against the menu's ACTUAL height. Always-present items:
              // "Open in local calendar" + "Ignore" (~72px). With a link it also
              // carries Open in browser + Copy link + a divider (+~73px). A flat
              // value let the link-bearing menu overflow off the bottom on a
              // near-bottom right-click. ~32px/item + 8px padding + 9px divider.
              top: Math.max(8, Math.min(menu.y, window.innerHeight - (menu.link ? 145 : 72))),
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
                    if (url && isWebUrl(url)) {
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
            {/* Always available: a calendar event always exists (unlike a video
                link). Opens the event in macOS Calendar.app via the ical://
                ekevent deep-link (backend command). */}
            <button
              onClick={() => {
                const eventId = menu.eventId;
                setMenu(null);
                void invoke("open_in_calendar", { eventId }).catch((e) =>
                  console.warn("open in local calendar:", e),
                );
              }}
              style={menuItemStyle}
            >
              Open in local calendar
            </button>
            <button onClick={() => toggleIgnore(menu.key, menu.endsAt)} style={menuItemStyle}>
              {ignored.has(menu.key) ? "Stop ignoring this event" : "Ignore this event"}
            </button>
          </div>
        </>
      )}
    </main>
  );
}

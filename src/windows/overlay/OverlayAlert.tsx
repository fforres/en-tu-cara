// Fullscreen takeover alert (Phase 5 + themes). Colors come from the active
// THEME (themes.ts) — always fixed rgba, identical on every display regardless
// of window activation (hard-won lesson; do not use CSS system colors here).

import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { extractMeetingLink, isWebUrl } from "../../lib/meeting-links";
import { resolveTheme, type Theme } from "./themes";
import type { UiEvent } from "../tray/TrayPopover";
import { accountsForEvent, type AccountInfo } from "../../lib/accounts";
import { capture } from "../../telemetry";
import { mockOverlayData } from "../tray/preview-data";

// DEV preview (ENTUCARA_PREVIEW=overlay → ?preview=1): render the takeover in a
// normal window seeded with mock alarms/events/calendars, so the layout and the
// "Calendar origins" section can be checked WITHOUT triggering a real full-screen
// takeover on the user's machine.
const PREVIEW = new URLSearchParams(window.location.search).get("preview") === "1";

interface AlarmPayload {
  occurrence_key: string;
  // Reminder kinds are tagged by offset (`reminder_5`, `reminder_20`, …); the
  // `string & {}` keeps literal hints without collapsing the union to `string`.
  kind: "t_zero" | "snooze" | (string & {});
  title: string;
  start: string | null;
  end: string | null;
}

interface CalendarInfo extends AccountInfo {
  id: string;
}

interface SettingsLite {
  theme: string;
  default_snooze_minutes: number;
}

function countdownLabel(payload: AlarmPayload, now: Date): string {
  if (!payload.start) {
    return "";
  }
  const ms = new Date(payload.start).getTime() - now.getTime();
  if (ms <= 0) {
    return "started";
  }
  const totalSec = Math.ceil(ms / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `starts in ${m}:${String(s).padStart(2, "0")}`;
}

function timeRange(alarm: AlarmPayload): string {
  if (!alarm.start) {
    return "";
  }
  const fmt: Intl.DateTimeFormatOptions = { hour: "numeric", minute: "2-digit" };
  const start = new Date(alarm.start).toLocaleTimeString(undefined, fmt);
  const end = alarm.end ? ` – ${new Date(alarm.end).toLocaleTimeString(undefined, fmt)}` : "";
  return `${start}${end}`;
}

export function OverlayAlert() {
  // Secondary displays render tint-only; the card shows once, on the primary.
  const role = new URLSearchParams(window.location.search).get("role") ?? "main";
  const [alarms, setAlarms] = useState<AlarmPayload[]>([]);
  const [events, setEvents] = useState<UiEvent[]>([]);
  const [calendars, setCalendars] = useState<Map<string, CalendarInfo>>(new Map());
  const [theme, setTheme] = useState<Theme>(() => resolveTheme(null));
  const [snoozeMinutes, setSnoozeMinutes] = useState<number>(5);
  const [now, setNow] = useState(() => new Date());
  const containerRef = useRef<HTMLElement>(null);

  useEffect(() => {
    // The fire emit happens BEFORE this window's JS boots — pull active alarms
    // on mount, and listen for any that fire while we're already visible.
    const dedupAdd = (incoming: AlarmPayload[]) =>
      setAlarms((prev) => {
        const seen = new Set(prev.map((a) => `${a.occurrence_key}#${a.kind}`));
        return [...prev, ...incoming.filter((a) => !seen.has(`${a.occurrence_key}#${a.kind}`))];
      });
    if (PREVIEW) {
      const mock = mockOverlayData(Date.now());
      setAlarms(mock.alarms);
      setEvents(mock.events);
      setCalendars(new Map(mock.calendars.map((c) => [c.id, c])));
      const clock = setInterval(() => setNow(new Date()), 1000);
      return () => clearInterval(clock);
    }
    invoke<AlarmPayload[]>("get_active_alarms")
      .then(dedupAdd)
      .catch(() => {});
    // Accounts for the "Calendar origins" section resolve from the calendar list
    // (account lives on the calendar, not the event). Best-effort: if this fails
    // the section just doesn't render — it never blocks the alert.
    invoke<CalendarInfo[]>("list_calendars")
      .then((cals) => setCalendars(new Map(cals.map((c) => [c.id, c]))))
      .catch(() => {});
    const unlistenPromise = listen<AlarmPayload>("alarm-fired", (e) => dedupAdd([e.payload]));
    // After a per-occurrence dismiss/snooze the backend keeps the overlay open
    // and emits the reduced set so the remaining cards (e.g. an overlapping
    // meeting) stay visible. Replace, don't append.
    const unlistenUpdated = listen<AlarmPayload[]>("alarms-updated", (e) => setAlarms(e.payload));
    // The Join link is resolved from a SEPARATE fetch_events (the alarm payload
    // carries no URL). A transient EventKit blip here used to leave events empty
    // for the life of the overlay → no Join button, exactly when the user most
    // needs it (M4). Retry a FAILED read a few times with a short backoff; a
    // successful read (even an empty one) is authoritative and ends the retries.
    let eventsCancelled = false;
    const fetchEventsWithRetry = (attempt: number) => {
      invoke<UiEvent[]>("fetch_events", { daysBack: 1, daysForward: 1 })
        .then((evs) => {
          if (!eventsCancelled) {
            setEvents(evs);
          }
        })
        .catch(() => {
          if (!eventsCancelled && attempt < 5) {
            setTimeout(() => fetchEventsWithRetry(attempt + 1), 1000);
          }
        });
    };
    fetchEventsWithRetry(0);
    invoke<SettingsLite>("get_settings")
      .then((s) => {
        setTheme(resolveTheme(s.theme));
        if (s.default_snooze_minutes && s.default_snooze_minutes > 0) {
          setSnoozeMinutes(s.default_snooze_minutes);
        }
      })
      .catch(() => {});
    const clock = setInterval(() => setNow(new Date()), 1000);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        void invoke("dismiss_alarms");
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      eventsCancelled = true;
      void unlistenPromise.then((u) => u());
      void unlistenUpdated.then((u) => u());
      clearInterval(clock);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const cards = useMemo(() => {
    return alarms.map((alarm) => {
      const event = events.find((e) => e.occurrence_key === alarm.occurrence_key);
      const link = event ? extractMeetingLink(event) : null;
      // The synced accounts this meeting is present on (account-level, so a
      // duplicate across subscribed colleague calendars under one account reads
      // as one origin). Empty until the calendar list resolves.
      const accounts = event ? accountsForEvent(event.calendars, calendars) : [];
      return { alarm, event, link, accounts };
    });
  }, [alarms, events, calendars]);

  // Key the focus effect on the card IDENTITY signature, not the count: a swap
  // that keeps the count the same (one dismissed + one added in a single
  // alarms-updated) must still re-run the effect, or focus strands on a removed
  // Dismiss node and the "stray Enter can't join" guarantee lapses.
  const cardsSignature = cards.map((c) => `${c.alarm.occurrence_key}#${c.alarm.kind}`).join("|");

  // Deterministic focus as cards arrive/leave (React's static autoFocus only
  // fires on a node's first mount, so it stranded focus when cards mounted after
  // the zero-card fallback). Land focus on the first Dismiss — NEVER Join, so a
  // stray Enter can't join a meeting. Don't steal focus if the user already
  // tabbed to one of our buttons. Esc works regardless (window-level listener).
  useEffect(() => {
    const root = containerRef.current;
    if (!root) {
      return;
    }
    const active = document.activeElement;
    if (active && active !== document.body && root.contains(active)) {
      return;
    }
    root.querySelector<HTMLButtonElement>("[data-dismiss]")?.focus();
  }, [cardsSignature]);

  if (role === "dim") {
    // Tint-only companion. pointer-events none + a window class that can never
    // become key: clicks neither interact NOR steal focus from the main alert.
    return <main style={{ height: "100%", background: theme.backdrop, pointerEvents: "none" }} />;
  }

  const secondaryButton: React.CSSProperties = {
    font: "inherit",
    padding: "10px 28px",
    borderRadius: 8,
    border: `1px solid ${theme.buttonBorder}`,
    background: "transparent",
    color: theme.text,
    cursor: "pointer",
  };

  return (
    <main
      ref={containerRef}
      // Scroll container: many/large cards (several overlapping meetings, the
      // 3-account origins list) overflow the screen and were CLIPPED — and flex
      // centering made the top unreachable. Scroll instead, with the scrollbar
      // hidden (trackpad/wheel still scrolls) so the takeover stays clean.
      className="overlay-scroll"
      style={{
        font: "17px system-ui, -apple-system, sans-serif",
        background: theme.backdrop,
        color: theme.text,
        height: "100%",
        overflowY: "auto",
        scrollbarWidth: "none",
      }}
    >
      <style>{`.overlay-scroll::-webkit-scrollbar{display:none}`}</style>
      {/* min-height:100% centers the content when it's short, but lets it grow
          and scroll from the TOP when it's tall (plain justify-content:center
          would clip the top of overflowing content). */}
      <div
        style={{
          minHeight: "100%",
          boxSizing: "border-box",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          flexDirection: "column",
          gap: 20,
          padding: "48px 24px",
          userSelect: "none",
        }}
      >
        <div style={{ fontSize: 64 }}>⏰</div>

        {cards.length === 0 && (
          <>
            <h1 style={{ fontWeight: 600 }}>Meeting starting…</h1>
            {/* Dismiss must ALWAYS exist (CP1b-human: "I couldn't close it"). No
              key → dismiss everything (the blunt escape hatch). */}
            <button
              data-dismiss
              onClick={() => void invoke("dismiss_alarms").catch(() => {})}
              style={secondaryButton}
            >
              Dismiss
            </button>
          </>
        )}

        {cards.map(({ alarm, link, accounts }) => (
          <section
            key={`${alarm.occurrence_key}#${alarm.kind}`}
            style={{
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 10,
              padding: "24px 40px",
              borderRadius: 14,
              background: theme.cardBg,
              maxWidth: 720,
            }}
          >
            <h1 style={{ margin: 0, fontSize: 34, fontWeight: 700, textAlign: "center" }}>
              {alarm.title || "Untitled event"}
            </h1>
            <div style={{ fontSize: 18, color: theme.textSecondary }}>{timeRange(alarm)}</div>
            <div style={{ fontSize: 20, fontVariantNumeric: "tabular-nums" }}>
              {countdownLabel(alarm, now)}
            </div>

            {accounts.length > 0 && (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: 6,
                  marginTop: 6,
                }}
              >
                <div
                  style={{
                    fontSize: 12,
                    letterSpacing: 0.8,
                    textTransform: "uppercase",
                    color: theme.textSecondary,
                  }}
                >
                  {accounts.length > 1 ? "Calendar origins" : "Calendar origin"}
                </div>
                <div
                  style={{ display: "flex", flexWrap: "wrap", gap: 6, justifyContent: "center" }}
                >
                  {accounts.map((account) => (
                    <span
                      key={account}
                      style={{
                        fontSize: 14,
                        padding: "3px 12px",
                        borderRadius: 999,
                        border: `1px solid ${theme.buttonBorder}`,
                        color: theme.text,
                      }}
                    >
                      {account}
                    </span>
                  ))}
                </div>
              </div>
            )}

            <div style={{ display: "flex", gap: 12, marginTop: 8 }}>
              {link && (
                <button
                  onClick={async () => {
                    const key = alarm.occurrence_key;
                    // Join is the one action Rust can't observe (it opens a URL,
                    // then dismisses). Record it here so we can tell joins apart
                    // from plain dismissals.
                    capture("alarm_joined");
                    try {
                      if (isWebUrl(link.url)) {
                        await openUrl(link.url);
                      }
                    } catch {
                      // Opening failed — still dismiss so the alert never sticks.
                    } finally {
                      void invoke("dismiss_alarms", { occurrenceKey: key }).catch(() => {});
                    }
                  }}
                  style={{
                    font: "inherit",
                    fontWeight: 600,
                    padding: "10px 28px",
                    borderRadius: 8,
                    border: "none",
                    background: theme.accent,
                    color: theme.accentText,
                    cursor: "pointer",
                  }}
                >
                  📹 Join
                </button>
              )}
              <button
                data-dismiss
                onClick={() =>
                  void invoke("dismiss_alarms", { occurrenceKey: alarm.occurrence_key }).catch(
                    () => {},
                  )
                }
                style={secondaryButton}
              >
                Dismiss
              </button>
            </div>

            <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
              <button
                onClick={() =>
                  void invoke("snooze_alarm", {
                    occurrenceKey: alarm.occurrence_key,
                    minutes: snoozeMinutes,
                  }).catch(() => {})
                }
                style={{
                  font: "inherit",
                  fontSize: 13,
                  padding: "5px 14px",
                  borderRadius: 6,
                  border: `1px solid ${theme.buttonBorder}`,
                  background: "transparent",
                  color: theme.text,
                  opacity: 0.85,
                  cursor: "pointer",
                }}
              >
                Remind me again in {snoozeMinutes} {snoozeMinutes === 1 ? "minute" : "minutes"}
              </button>
            </div>
          </section>
        ))}
      </div>
    </main>
  );
}

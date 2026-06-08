// Fullscreen takeover alert (Phase 5 + themes). Colors come from the active
// THEME (themes.ts) — always fixed rgba, identical on every display regardless
// of window activation (hard-won lesson; do not use CSS system colors here).

import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { extractMeetingLink } from "../../lib/meeting-links";
import { resolveTheme, type Theme } from "./themes";
import type { UiEvent } from "../tray/TrayPopover";

/// Belt-and-suspenders before handing a calendar-derived string to the OS opener:
/// only ever open http(s). The extractor already anchors to https?://, but a
/// malicious invite must never coax us into a javascript:/file:/custom scheme.
function isWebUrl(raw: string): boolean {
  try {
    const p = new URL(raw).protocol;
    return p === "https:" || p === "http:";
  } catch {
    return false;
  }
}

interface AlarmPayload {
  occurrence_key: string;
  // `string & {}` keeps literal hints without collapsing the union to `string`.
  kind: "t_minus5" | "t_zero" | "snooze" | (string & {});
  title: string;
  start: string | null;
  end: string | null;
}

interface SettingsLite {
  theme: string;
  snooze_minutes: number[];
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
  const [theme, setTheme] = useState<Theme>(() => resolveTheme(null));
  const [snoozes, setSnoozes] = useState<number[]>([1, 5]);
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
    invoke<AlarmPayload[]>("get_active_alarms")
      .then(dedupAdd)
      .catch(() => {});
    const unlistenPromise = listen<AlarmPayload>("alarm-fired", (e) => dedupAdd([e.payload]));
    // After a per-occurrence dismiss/snooze the backend keeps the overlay open
    // and emits the reduced set so the remaining cards (e.g. an overlapping
    // meeting) stay visible. Replace, don't append.
    const unlistenUpdated = listen<AlarmPayload[]>("alarms-updated", (e) => setAlarms(e.payload));
    invoke<UiEvent[]>("fetch_events", { daysBack: 1, daysForward: 1 })
      .then(setEvents)
      .catch(() => {});
    invoke<SettingsLite>("get_settings")
      .then((s) => {
        setTheme(resolveTheme(s.theme));
        if (s.snooze_minutes?.length) {
          setSnoozes(s.snooze_minutes);
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
      return { alarm, event, link };
    });
  }, [alarms, events]);

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
  }, [cards.length]);

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
      style={{
        font: "17px system-ui, -apple-system, sans-serif",
        background: theme.backdrop,
        color: theme.text,
        height: "100%",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flexDirection: "column",
        gap: 20,
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

      {cards.map(({ alarm, link }) => (
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

          <div style={{ display: "flex", gap: 12, marginTop: 8 }}>
            {link && (
              <button
                onClick={async () => {
                  const key = alarm.occurrence_key;
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
            {snoozes.map((m) => (
              <button
                key={m}
                onClick={() =>
                  void invoke("snooze_alarm", {
                    occurrenceKey: alarm.occurrence_key,
                    minutes: m,
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
                Snooze {m} min
              </button>
            ))}
          </div>
        </section>
      ))}
    </main>
  );
}

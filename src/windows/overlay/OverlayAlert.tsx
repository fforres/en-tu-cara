// Fullscreen takeover alert (Phase 5). Native-feeling: system font, system
// colors, no decoration beyond what the moment needs: title, time, countdown,
// Join when a link exists, Snooze 1m/5m, Dismiss.

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { extractMeetingLink } from "../../lib/meeting-links";
import type { UiEvent } from "../tray/TrayPopover";

interface AlarmPayload {
  occurrence_key: string;
  // `string & {}` keeps the literal hints for autocomplete without collapsing the union to `string`.
  kind: "t_minus5" | "t_zero" | "snooze" | (string & {});
  title: string;
  start: string | null;
  end: string | null;
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

// Identical on EVERY display, active or not — system colors (Canvas) resolve
// per-window appearance and made each monitor a different shade (CP1b-human #2).
const BACKDROP = "rgba(22, 22, 26, 0.55)";
const CARD_BG = "rgba(30, 30, 34, 0.92)";
const TEXT = "rgba(255, 255, 255, 0.95)";

export function OverlayAlert() {
  // Secondary displays render frosted glass only — the card shows once, on the
  // primary display (CP1b-human feedback). The native NSVisualEffectView behind
  // this transparent webview provides the actual blur; we only tint it.
  const role = new URLSearchParams(window.location.search).get("role") ?? "main";
  const [alarms, setAlarms] = useState<AlarmPayload[]>([]);
  const [events, setEvents] = useState<UiEvent[]>([]);
  const [now, setNow] = useState(() => new Date());

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
    invoke<UiEvent[]>("fetch_events", { daysBack: 1, daysForward: 1 })
      .then(setEvents)
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

  if (role === "dim") {
    // Tint-only companion panel. Deliberately NOT clickable — dismissal happens
    // only via the explicit buttons on the primary display (user request:
    // accidental clicks must never kill an alarm).
    return <main style={{ height: "100%", background: BACKDROP }} />;
  }

  return (
    <main
      style={{
        font: "17px system-ui, -apple-system, sans-serif",
        background: BACKDROP,
        color: TEXT,
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
          {/* Spike/edge path renders no cards — Dismiss must ALWAYS exist
              (CP1b-human: "I couldn't close it"). Esc works too. */}
          <button
            autoFocus
            onClick={() => invoke("dismiss_alarms")}
            style={{
              font: "inherit",
              padding: "10px 28px",
              borderRadius: 8,
              border: "1px solid rgba(255,255,255,0.3)",
              background: "rgba(255,255,255,0.1)",
              color: TEXT,
              cursor: "pointer",
            }}
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
            background: CARD_BG,
            maxWidth: 720,
          }}
        >
          <h1 style={{ margin: 0, fontSize: 34, fontWeight: 700, textAlign: "center" }}>
            {alarm.title || "Untitled event"}
          </h1>
          <div style={{ fontSize: 18, opacity: 0.75 }}>
            {alarm.start &&
              `${new Date(alarm.start).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}${
                alarm.end
                  ? ` – ${new Date(alarm.end).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}`
                  : ""
              }`}
          </div>
          <div style={{ fontSize: 20, fontVariantNumeric: "tabular-nums" }}>
            {countdownLabel(alarm, now)}
          </div>

          <div style={{ display: "flex", gap: 12, marginTop: 8 }}>
            {link && (
              <button
                autoFocus
                onClick={async () => {
                  await openUrl(link.url);
                  void invoke("dismiss_alarms");
                }}
                style={{
                  font: "inherit",
                  fontWeight: 600,
                  padding: "10px 28px",
                  borderRadius: 8,
                  border: "none",
                  background: "#3478f6",
                  color: "#fff",
                  cursor: "pointer",
                }}
              >
                📹 Join
              </button>
            )}
            <button
              onClick={() => invoke("dismiss_alarms")}
              style={{
                font: "inherit",
                padding: "10px 28px",
                borderRadius: 8,
                border: "1px solid rgba(255,255,255,0.3)",
                background: "transparent",
                color: TEXT,
                cursor: "pointer",
              }}
            >
              Dismiss
            </button>
          </div>

          <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
            {[1, 5].map((m) => (
              <button
                key={m}
                onClick={() =>
                  invoke("snooze_alarm", { occurrenceKey: alarm.occurrence_key, minutes: m })
                }
                style={{
                  font: "inherit",
                  fontSize: 13,
                  padding: "5px 14px",
                  borderRadius: 6,
                  border: "1px solid rgba(255,255,255,0.25)",
                  background: "transparent",
                  color: TEXT,
                  opacity: 0.8,
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

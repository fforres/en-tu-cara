// Settings window — VS Code style: sidebar with fuzzy search +
// section TOC; content renders matching settings with label highlights.
// Raw macOS styling: system-ui, CSS system colors (opaque normal window — the
// active/inactive system-color trap only applies to the overlay panels).

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  REGISTRY,
  SECTIONS,
  type Control,
  type SectionId,
  type SettingDef,
  type Settings,
} from "./registry";
import { searchSettings } from "./fuzzy";
import { THEMES } from "../overlay/themes";
import { checkForUpdate, installAndRelaunch } from "../../lib/updater";
import type { Update } from "@tauri-apps/plugin-updater";
import { capture, initTelemetry, setTelemetryEnabled } from "../../telemetry";

interface CalendarInfo {
  id: string;
  title: string;
  account: string | null;
  color: [number, number, number, number] | null;
}

const css = {
  hairline: "1px solid color-mix(in srgb, CanvasText 12%, transparent)",
  secondary: "color-mix(in srgb, CanvasText 55%, transparent)",
} as const;

function Highlight({ text, ranges }: { text: string; ranges: Array<[number, number]> }) {
  if (!ranges.length) {
    return <>{text}</>;
  }
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  ranges.forEach(([start, end]) => {
    if (start > cursor) {
      parts.push(text.slice(cursor, start));
    }
    parts.push(
      <mark
        key={`${start}-${end}`}
        style={{ background: "Highlight", color: "HighlightText", borderRadius: 2 }}
      >
        {text.slice(start, end)}
      </mark>,
    );
    cursor = end;
  });
  parts.push(text.slice(cursor));
  return <>{parts}</>;
}

function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <input
      type="checkbox"
      role="switch"
      aria-label={label}
      checked={checked}
      onChange={(e) => onChange(e.target.checked)}
      style={{ width: 18, height: 18, accentColor: "AccentColor" }}
    />
  );
}

type CheckState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "uptodate" }
  | { kind: "available"; version: string; update: Update }
  | { kind: "installing" }
  | { kind: "error" };

// App version + a Check-for-Updates button (About tab). Its own component so the
// check state can use hooks (a switch case can't). __APP_VERSION__ is baked by
// Vite; the updater commands run here because the settings window holds the
// updater capability (see capabilities/updater.json, windows: ["*"]).
function VersionControl() {
  const [state, setState] = useState<CheckState>({ kind: "idle" });

  const check = useCallback(async () => {
    setState({ kind: "checking" });
    const result = await checkForUpdate();
    if (result.status === "available") {
      setState({ kind: "available", version: result.version, update: result.update });
    } else if (result.status === "none") {
      setState({ kind: "uptodate" });
    } else {
      setState({ kind: "error" });
    }
  }, []);

  const install = useCallback(async (update: Update) => {
    setState({ kind: "installing" });
    try {
      await installAndRelaunch(update);
    } catch {
      setState({ kind: "error" });
    }
  }, []);

  const busy = state.kind === "checking" || state.kind === "installing";
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 10 }}>
      <span style={{ fontVariantNumeric: "tabular-nums" }}>v{__APP_VERSION__}</span>
      {state.kind === "available" ? (
        <button
          onClick={() => void install(state.update)}
          style={{ font: "inherit", padding: "3px 10px", cursor: "pointer" }}
        >
          Update to v{state.version} &amp; restart
        </button>
      ) : (
        <button
          onClick={() => void check()}
          disabled={busy}
          style={{ font: "inherit", padding: "3px 10px", cursor: "pointer" }}
        >
          {state.kind === "checking"
            ? "Checking…"
            : state.kind === "installing"
              ? "Installing…"
              : "Check for Updates"}
        </button>
      )}
      {state.kind === "uptodate" && (
        <span style={{ color: css.secondary, fontSize: 12 }}>You're up to date</span>
      )}
      {state.kind === "error" && (
        <span style={{ color: css.secondary, fontSize: 12 }}>Couldn't check</span>
      )}
    </span>
  );
}

// Suggestion box → PostHog (Rust submit_feedback). Explicit, opt-in send that
// works even when usage telemetry is off; optional email for a reply.
function FeedbackControl() {
  const [message, setMessage] = useState("");
  const [email, setEmail] = useState("");
  const [state, setState] = useState<"idle" | "sending" | "sent" | "error">("idle");

  const send = useCallback(async () => {
    if (!message.trim()) {
      return;
    }
    setState("sending");
    try {
      await invoke("submit_feedback", { message, email: email.trim() || null });
      setMessage("");
      setEmail("");
      setState("sent");
    } catch {
      setState("error");
    }
  }, [message, email]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, width: "100%", maxWidth: 420 }}>
      <textarea
        aria-label="Your suggestion"
        placeholder="What would make En Tu Cara better?"
        value={message}
        onChange={(e) => {
          setMessage(e.target.value);
          if (state !== "idle") {
            setState("idle");
          }
        }}
        rows={4}
        style={{ font: "inherit", padding: "6px 8px", resize: "vertical" }}
      />
      <input
        type="email"
        aria-label="Your email (optional)"
        placeholder="Email (optional, for a reply)"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        style={{ font: "inherit", padding: "4px 8px" }}
      />
      <span style={{ display: "inline-flex", alignItems: "center", gap: 10 }}>
        <button
          onClick={() => void send()}
          disabled={state === "sending" || !message.trim()}
          style={{ font: "inherit", padding: "4px 14px", cursor: "pointer" }}
        >
          {state === "sending" ? "Sending…" : "Send"}
        </button>
        {state === "sent" && (
          <span style={{ color: css.secondary, fontSize: 12 }}>Thanks — sent! 🙏</span>
        )}
        {state === "error" && (
          <span style={{ color: css.secondary, fontSize: 12 }}>Couldn't send — try again.</span>
        )}
      </span>
    </div>
  );
}

// Export local logs → Downloads + clipboard (Rust export_logs). For handing
// diagnostics to a maintainer when something breaks.
function ExportLogsControl() {
  const [state, setState] = useState<
    { kind: "idle" | "exporting" | "error" } | { kind: "done"; path: string }
  >({ kind: "idle" });

  const run = useCallback(async () => {
    setState({ kind: "exporting" });
    try {
      const path = await invoke<string>("export_logs");
      setState({ kind: "done", path });
    } catch {
      setState({ kind: "error" });
    }
  }, []);

  return (
    <span
      style={{ display: "inline-flex", flexDirection: "column", gap: 6, alignItems: "flex-start" }}
    >
      <button
        onClick={() => void run()}
        disabled={state.kind === "exporting"}
        style={{ font: "inherit", padding: "4px 12px", cursor: "pointer" }}
      >
        {state.kind === "exporting" ? "Exporting…" : "Export logs"}
      </button>
      {state.kind === "done" && (
        <span style={{ color: css.secondary, fontSize: 12, userSelect: "text" }}>
          Saved to {state.path} (and copied to clipboard)
        </span>
      )}
      {state.kind === "error" && (
        <span style={{ color: css.secondary, fontSize: 12 }}>Couldn't export logs.</span>
      )}
    </span>
  );
}

function ControlView({
  def,
  settings,
  calendars,
  calStatus,
  onGrantCalendar,
  sounds,
  update,
}: {
  def: SettingDef;
  settings: Settings;
  calendars: CalendarInfo[];
  calStatus: string;
  onGrantCalendar: () => void;
  sounds: string[];
  update: (patch: Partial<Settings>) => void;
}) {
  const control: Control = def.control;

  switch (control.kind) {
    case "toggle": {
      const value = Boolean(settings[control.key]);
      return (
        <Toggle checked={value} onChange={(v) => update({ [control.key]: v })} label={def.label} />
      );
    }
    case "number": {
      const value = Number(settings[control.key]);
      return (
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
          <input
            type="number"
            aria-label={def.label}
            min={control.min}
            max={control.max}
            value={value}
            onChange={(e) => {
              const n = Math.min(
                control.max,
                Math.max(control.min, Number(e.target.value) || control.min),
              );
              update({ [control.key]: n });
            }}
            style={{ width: 64, font: "inherit", padding: "3px 6px" }}
          />
          <span style={{ color: css.secondary, fontSize: 12 }}>{control.unit}</span>
        </span>
      );
    }
    case "sound": {
      return (
        <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
          <select
            aria-label={def.label}
            value={settings.alert_sound}
            onChange={(e) => {
              update({ alert_sound: e.target.value });
              void invoke("preview_sound", { name: e.target.value });
            }}
            style={{ font: "inherit", padding: "3px 6px", minWidth: 140 }}
          >
            {(sounds.length ? sounds : [settings.alert_sound]).map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
          <button
            onClick={() => void invoke("preview_sound", { name: settings.alert_sound })}
            title="Preview"
            style={{ font: "inherit", padding: "3px 10px", cursor: "pointer" }}
          >
            ▶
          </button>
        </span>
      );
    }
    case "select": {
      const value = String(settings[control.key]);
      return (
        <select
          aria-label={def.label}
          value={value}
          onChange={(e) => update({ [control.key]: e.target.value })}
          style={{ font: "inherit", padding: "3px 6px", minWidth: 140 }}
        >
          {control.options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      );
    }
    case "snooze-list": {
      return (
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
          {settings.snooze_minutes.map((m, i) => (
            <input
              key={i}
              type="number"
              aria-label={`Snooze duration ${i + 1}`}
              min={1}
              max={120}
              value={m}
              onChange={(e) => {
                const next = [...settings.snooze_minutes];
                next[i] = Math.min(120, Math.max(1, Number(e.target.value) || 1));
                update({ snooze_minutes: next });
              }}
              style={{ width: 56, font: "inherit", padding: "3px 6px" }}
            />
          ))}
          <span style={{ color: css.secondary, fontSize: 12 }}>minutes</span>
        </span>
      );
    }
    case "calendar-list": {
      // Gate the list behind calendar authorization — offer a way to grant it.
      if (calStatus !== "FullAccess") {
        const denied = calStatus === "Denied" || calStatus === "Restricted";
        return (
          <div
            style={{ display: "flex", flexDirection: "column", gap: 8, alignItems: "flex-start" }}
          >
            <span style={{ color: css.secondary, fontSize: 12, maxWidth: 360 }}>
              {denied
                ? "Calendar access is turned off. Enable En Tu Cara under System Settings → Privacy & Security → Calendars, then reopen this window."
                : "En Tu Cara needs access to your calendars to alert you about meetings."}
            </span>
            {!denied && (
              <button
                onClick={onGrantCalendar}
                style={{ font: "inherit", padding: "4px 12px", cursor: "pointer" }}
              >
                Grant calendar access
              </button>
            )}
          </div>
        );
      }
      const enabled = settings.enabled_calendar_ids; // null = all
      const isOn = (id: string) => enabled === null || enabled.includes(id);
      const setCal = (id: string, on: boolean) => {
        const allIds = calendars.map((c) => c.id);
        const current = enabled === null ? allIds : enabled;
        const next = on ? [...new Set([...current, id])] : current.filter((c) => c !== id);
        // Everything on → store null ("all", future calendars auto-included).
        update({ enabled_calendar_ids: next.length === allIds.length ? null : next });
      };
      const byAccount = new Map<string, CalendarInfo[]>();
      calendars.forEach((c) => {
        const key = c.account ?? "Other";
        byAccount.set(key, [...(byAccount.get(key) ?? []), c]);
      });
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: 10, width: "100%" }}>
          {[...byAccount.entries()].map(([account, cals]) => (
            <div key={account}>
              <div
                style={{
                  fontSize: 11,
                  fontWeight: 700,
                  color: css.secondary,
                  textTransform: "uppercase",
                  marginBottom: 4,
                }}
              >
                {account}
              </div>
              {cals.map((c) => (
                <label
                  key={c.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    padding: "3px 0",
                    cursor: "pointer",
                  }}
                >
                  <input
                    type="checkbox"
                    checked={isOn(c.id)}
                    onChange={(e) => setCal(c.id, e.target.checked)}
                    style={{
                      accentColor: c.color
                        ? `rgba(${Math.round(c.color[0] * 255)}, ${Math.round(c.color[1] * 255)}, ${Math.round(c.color[2] * 255)}, ${c.color[3]})`
                        : undefined,
                    }}
                  />
                  <span style={{ fontSize: 13 }}>{c.title}</span>
                </label>
              ))}
            </div>
          ))}
          {calendars.length === 0 && (
            <span style={{ color: css.secondary, fontSize: 12 }}>No calendars found.</span>
          )}
        </div>
      );
    }
    case "theme": {
      return (
        <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
          <select
            aria-label={def.label}
            value={settings.theme}
            onChange={(e) => update({ theme: e.target.value })}
            style={{ font: "inherit", padding: "3px 6px", minWidth: 140 }}
          >
            {THEMES.map((t) => (
              <option key={t.id} value={t.id}>
                {t.label}
              </option>
            ))}
          </select>
          <button
            onClick={() => void invoke("demo_alert")}
            style={{ font: "inherit", padding: "3px 12px", cursor: "pointer" }}
          >
            Show Demo Alert
          </button>
        </span>
      );
    }
    case "link": {
      return (
        <button
          onClick={() => void invoke("open_url", { url: control.url })}
          style={{ font: "inherit", padding: "4px 12px", cursor: "pointer" }}
        >
          {control.button}
        </button>
      );
    }
    case "feedback": {
      return <FeedbackControl />;
    }
    case "export-logs": {
      return <ExportLogsControl />;
    }
    case "version": {
      return <VersionControl />;
    }
    case "note": {
      // Description-only row (the blurb lives in def.description).
      return null;
    }
    case "placeholder": {
      return (
        <span style={{ color: css.secondary, fontSize: 12, fontStyle: "italic" }}>
          {control.note}
        </span>
      );
    }
  }
}

export function SettingsWindow() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [calendars, setCalendars] = useState<CalendarInfo[]>([]);
  const [calStatus, setCalStatus] = useState<string>("NotDetermined");
  const [sounds, setSounds] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [activeSection, setActiveSection] = useState<SectionId>(() => {
    const fromUrl = new URLSearchParams(window.location.search).get("section");
    return SECTIONS.some((s) => s.id === fromUrl) ? (fromUrl as SectionId) : "general";
  });
  const [error, setError] = useState<string | null>(null);
  // Calendar-access health, surfaced as a banner. The Rust scheduler emits
  // `access-state-changed` on the Ok↔Lost edge; we also pull on mount in case
  // the window opened AFTER the transition (the overlay's pull-and-listen
  // pattern). This is the loud, in-app half of "never silently stop alerting."
  // `accessReason` differentiates the two loss modes (they need different CTAs):
  // a revoked grant → the user must re-grant; reads failing DESPITE a grant
  // (the poisoned-TCC-record incident) → the app repairs itself, with a manual
  // "Repair" escape hatch.
  const [accessLost, setAccessLost] = useState(false);
  const [accessReason, setAccessReason] = useState("");
  // Mirror of `settings` so `update` can merge patches without a stale closure
  // and WITHOUT calling invoke inside a setState updater (which StrictMode runs
  // twice → double set_settings).
  const settingsRef = useRef<Settings | null>(null);
  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  const refreshCalendars = useCallback(() => {
    invoke<string>("calendar_authorization_status")
      .then(setCalStatus)
      .catch(() => {});
    invoke<CalendarInfo[]>("list_calendars")
      .then(setCalendars)
      .catch(() => setCalendars([]));
  }, []);

  // Trigger the macOS calendar prompt, then re-read status + calendars.
  const onGrantCalendar = useCallback(() => {
    invoke("request_calendar_access")
      .catch((e) => setError(String(e)))
      .finally(refreshCalendars);
  }, [refreshCalendars]);

  // Manual entry to the TCC grant repair (reset + fresh prompt) for the
  // "granted but reads fail" loss mode. Fire-and-forget: the Rust side does
  // everything off-thread and the access banner reflects the outcome live.
  const onRepairCalendar = useCallback(() => {
    invoke("repair_calendar_access").catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    invoke<Settings>("get_settings")
      .then(setSettings)
      .catch((e) => setError(String(e)));
    invoke<string[]>("list_system_sounds")
      .then(setSounds)
      .catch(() => {});
    refreshCalendars();
    // Telemetry may not have finished init when this window mounts; ensure it,
    // then record the open (no-op if telemetry is disabled).
    void initTelemetry().then(() => capture("settings_opened"));
  }, [refreshCalendars]);

  // Calendar-access banner: pull current state on mount + listen for live edges.
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
      if (e.payload.state === "ok") {
        refreshCalendars();
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [refreshCalendars]);

  const update = useCallback((patch: Partial<Settings>) => {
    const prev = settingsRef.current;
    if (!prev) {
      return;
    }
    const next = { ...prev, ...patch };
    settingsRef.current = next; // keep current for back-to-back updates
    setSettings(next);
    // Live-apply the telemetry toggle (opt in/out) the moment it changes.
    if (
      patch.telemetry_enabled !== undefined &&
      patch.telemetry_enabled !== prev.telemetry_enabled
    ) {
      void setTelemetryEnabled(patch.telemetry_enabled);
    }
    // Persist OUTSIDE the state updater (live-apply is the contract — no save
    // button). Clear any stale error banner once a write succeeds.
    invoke("set_settings", { settings: next })
      .then(() => setError(null))
      .catch((e) => setError(String(e)));
  }, []);

  const hits = useMemo(() => searchSettings(query, REGISTRY), [query]);
  const searching = query.trim().length > 0;
  const visible = searching ? hits : hits.filter((h) => h.setting.section === activeSection);
  const matchedSections = useMemo(() => new Set(hits.map((h) => h.setting.section)), [hits]);

  if (!settings) {
    return (
      <main
        style={{
          font: "13px system-ui",
          colorScheme: "light dark",
          background: "Canvas",
          color: "CanvasText",
          height: "100vh",
          display: "grid",
          placeItems: "center",
        }}
      >
        {error ?? "Loading…"}
      </main>
    );
  }

  return (
    <main
      style={{
        font: "13px system-ui, -apple-system, sans-serif",
        colorScheme: "light dark",
        background: "Canvas",
        color: "CanvasText",
        height: "100vh",
        display: "flex",
        userSelect: "none",
        overflow: "hidden",
      }}
    >
      {/* Sidebar: search + TOC */}
      <nav
        style={{
          width: 190,
          borderRight: css.hairline,
          display: "flex",
          flexDirection: "column",
          padding: 10,
          gap: 10,
        }}
      >
        <input
          type="search"
          placeholder="Search settings"
          aria-label="Search settings"
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          style={{
            font: "inherit",
            padding: "5px 8px",
            borderRadius: 6,
            border: css.hairline,
            background: "Field",
            color: "FieldText",
          }}
        />
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          {SECTIONS.map((section) => {
            const active = !searching && section.id === activeSection;
            const dimmed = searching && !matchedSections.has(section.id);
            return (
              <button
                key={section.id}
                onClick={() => {
                  setQuery("");
                  setActiveSection(section.id);
                }}
                style={{
                  font: "inherit",
                  textAlign: "left",
                  padding: "5px 8px",
                  borderRadius: 6,
                  border: "none",
                  cursor: "pointer",
                  background: active ? "Highlight" : "transparent",
                  color: active ? "HighlightText" : "CanvasText",
                  opacity: dimmed ? 0.35 : 1,
                }}
              >
                {section.label}
              </button>
            );
          })}
        </div>
      </nav>

      {/* Content */}
      <section style={{ flex: 1, overflowY: "auto", padding: "14px 20px" }}>
        {accessLost && (
          <div
            role="alert"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "10px 12px",
              marginBottom: 12,
              borderRadius: 8,
              background: "color-mix(in srgb, crimson 16%, Canvas)",
              border: "1px solid color-mix(in srgb, crimson 45%, transparent)",
            }}
          >
            {accessReason === "fetch_failed_despite_authorized" ? (
              <>
                <span style={{ fontSize: 13, flex: 1 }}>
                  ⚠️ <strong>Calendar stopped responding — alerts are paused.</strong> Access is
                  granted, but macOS is returning no events — the permission record may be
                  corrupted. En Tu Cara repairs this automatically; repairing shows a fresh access
                  prompt.
                </span>
                <button
                  onClick={onRepairCalendar}
                  style={{
                    font: "inherit",
                    fontWeight: 600,
                    padding: "5px 12px",
                    cursor: "pointer",
                  }}
                >
                  Repair access
                </button>
              </>
            ) : (
              <>
                <span style={{ fontSize: 13, flex: 1 }}>
                  ⚠️ <strong>Calendar access was lost — alerts are paused.</strong> En Tu Cara can't
                  read your calendar, so no meeting alerts will fire until access is restored.
                </span>
                <button
                  onClick={onGrantCalendar}
                  style={{
                    font: "inherit",
                    fontWeight: 600,
                    padding: "5px 12px",
                    cursor: "pointer",
                  }}
                >
                  Grant calendar access
                </button>
              </>
            )}
          </div>
        )}
        {error && <div style={{ color: "crimson", fontSize: 12, marginBottom: 8 }}>{error}</div>}
        {searching && (
          <div style={{ color: css.secondary, fontSize: 12, marginBottom: 10 }}>
            {visible.length} result{visible.length === 1 ? "" : "s"} for “{query.trim()}”
          </div>
        )}
        {visible.length === 0 && (
          <div style={{ color: css.secondary, padding: 24 }}>No settings match.</div>
        )}
        {visible.map(({ setting, labelRanges }) => (
          <div
            key={setting.id}
            data-setting-id={setting.id}
            style={{
              display: "flex",
              alignItems: "flex-start",
              gap: 16,
              padding: "12px 0",
              borderBottom: css.hairline,
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontWeight: 600 }}>
                <Highlight text={setting.label} ranges={labelRanges} />
                {searching && (
                  <span
                    style={{ fontWeight: 400, color: css.secondary, fontSize: 11, marginLeft: 8 }}
                  >
                    {SECTIONS.find((s) => s.id === setting.section)?.label}
                  </span>
                )}
              </div>
              <div style={{ color: css.secondary, fontSize: 12, marginTop: 2, userSelect: "text" }}>
                {setting.description}
              </div>
            </div>
            <div style={{ flexShrink: 0, paddingTop: 2, maxWidth: 380 }}>
              <ControlView
                def={setting}
                settings={settings}
                calendars={calendars}
                calStatus={calStatus}
                onGrantCalendar={onGrantCalendar}
                sounds={sounds}
                update={update}
              />
            </div>
          </div>
        ))}
      </section>
    </main>
  );
}

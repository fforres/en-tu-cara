// Settings window (PLAN Phase 7.3) — VS Code style: sidebar with fuzzy search +
// section TOC; content renders matching settings with label highlights.
// Raw macOS styling: system-ui, CSS system colors (opaque normal window — the
// active/inactive system-color trap only applies to the overlay panels).

import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  ranges.forEach(([start, end], i) => {
    if (start > cursor) {
      parts.push(text.slice(cursor, start));
    }
    parts.push(
      <mark key={i} style={{ background: "Highlight", color: "HighlightText", borderRadius: 2 }}>
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
      style={{ width: 18, height: 18, accentColor: "Highlight" }}
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

  useEffect(() => {
    invoke<Settings>("get_settings")
      .then(setSettings)
      .catch((e) => setError(String(e)));
    invoke<string[]>("list_system_sounds")
      .then(setSounds)
      .catch(() => {});
    refreshCalendars();
  }, [refreshCalendars]);

  const update = useCallback((patch: Partial<Settings>) => {
    setSettings((prev) => {
      if (!prev) {
        return prev;
      }
      const next = { ...prev, ...patch };
      invoke("set_settings", { settings: next }).catch((e) => setError(String(e)));
      return next;
    });
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

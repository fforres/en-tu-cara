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

function ControlView({
  def,
  settings,
  calendars,
  sounds,
  update,
}: {
  def: SettingDef;
  settings: Settings;
  calendars: CalendarInfo[];
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
            <span style={{ color: css.secondary, fontSize: 12 }}>
              No calendars (grant calendar access first).
            </span>
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
  const [sounds, setSounds] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [activeSection, setActiveSection] = useState<SectionId>(() => {
    const fromUrl = new URLSearchParams(window.location.search).get("section");
    return SECTIONS.some((s) => s.id === fromUrl) ? (fromUrl as SectionId) : "general";
  });
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<Settings>("get_settings")
      .then(setSettings)
      .catch((e) => setError(String(e)));
    invoke<CalendarInfo[]>("list_calendars")
      .then(setCalendars)
      .catch(() => {});
    invoke<string[]>("list_system_sounds")
      .then(setSounds)
      .catch(() => {});
  }, []);

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

// Settings registry (PLAN Phase 7): every setting is DATA — the sidebar TOC,
// the section views, and fuzzy search are all generated from this list.

export type SectionId =
  | "general"
  | "alerts"
  | "calendars"
  | "event-filters"
  | "menu-bar"
  | "appearance"
  | "advanced";

export const SECTIONS: Array<{ id: SectionId; label: string }> = [
  { id: "general", label: "General" },
  { id: "alerts", label: "Alerts" },
  { id: "calendars", label: "Calendars" },
  { id: "event-filters", label: "Event Filters" },
  { id: "menu-bar", label: "Menu Bar" },
  { id: "appearance", label: "Appearance" },
  { id: "advanced", label: "Advanced" },
];

/** Mirror of the Rust `Settings` struct (settings.rs). */
export interface Settings {
  enabled_calendar_ids: string[] | null;
  lead_minutes: number;
  alert_sound: string;
  sound_repeat_secs: number;
  snooze_minutes: number[];
  alert_tentative: boolean;
  alert_pending: boolean;
  only_video_events: boolean;
  show_all_day_in_tray: boolean;
  auto_close_enabled: boolean;
  auto_close_minutes: number;
  launch_at_login: boolean;
  show_next_event_in_menu_bar: boolean;
  menu_bar_title_chars: number;
  theme: string;
  /** Menu-bar tray icon style: "auto" (template, adapts to light/dark) | "light" | "dark". */
  tray_icon: string;
  /** First-run onboarding completed (not a user-facing setting; gates onboarding). */
  onboarded: boolean;
}

export type Control =
  | { kind: "toggle"; key: keyof Settings }
  | { kind: "number"; key: keyof Settings; min: number; max: number; unit: string }
  | { kind: "sound" } // sound picker + preview (alert_sound)
  | { kind: "snooze-list" } // snooze_minutes editor
  | { kind: "calendar-list" } // enabled_calendar_ids editor
  | { kind: "theme" } // theme picker + demo alert button
  | { kind: "select"; key: keyof Settings; options: Array<{ value: string; label: string }> }
  | { kind: "placeholder"; note: string }; // documented not-yet feature

export interface SettingDef {
  id: string;
  section: SectionId;
  label: string;
  description: string;
  /** Extra fuzzy-search bait beyond label/description words. */
  keywords: string[];
  control: Control;
}

export const REGISTRY: SettingDef[] = [
  // ── General ────────────────────────────────────────────────────────────
  {
    id: "general.launch-at-login",
    section: "general",
    label: "Start at login",
    description: "Launch En Tu Cara automatically when you log in.",
    keywords: ["startup", "boot", "autostart", "login item"],
    control: { kind: "toggle", key: "launch_at_login" },
  },
  // ── Alerts ─────────────────────────────────────────────────────────────
  {
    id: "alerts.lead-minutes",
    section: "alerts",
    label: "Alert me before the event",
    description:
      "How many minutes before a meeting the early alert appears. A second alert always fires at meeting start.",
    keywords: ["lead", "before", "early", "minutes", "t-5", "warning"],
    control: { kind: "number", key: "lead_minutes", min: 1, max: 60, unit: "min" },
  },
  {
    id: "alerts.sound",
    section: "alerts",
    label: "Alert sound",
    description: "System sound played while an alert is on screen.",
    keywords: ["audio", "chime", "ring", "sosumi", "volume"],
    control: { kind: "sound" },
  },
  {
    id: "alerts.sound-repeat",
    section: "alerts",
    label: "Repeat sound every",
    description: "The sound repeats at this interval until you dismiss, snooze, or join.",
    keywords: ["recurring", "repeat", "interval", "loop", "nag"],
    control: { kind: "number", key: "sound_repeat_secs", min: 2, max: 60, unit: "sec" },
  },
  {
    id: "alerts.snooze-durations",
    section: "alerts",
    label: "Snooze durations",
    description: "The snooze buttons shown on the alert, in minutes.",
    keywords: ["snooze", "delay", "postpone", "later"],
    control: { kind: "snooze-list" },
  },
  // ── Calendars ──────────────────────────────────────────────────────────
  {
    id: "calendars.active",
    section: "calendars",
    label: "Active calendars",
    description:
      "Only checked calendars trigger alerts and appear in the menu-bar list. Grouped by account.",
    keywords: ["accounts", "google", "icloud", "exchange", "enable", "disable", "mute", "holidays"],
    control: { kind: "calendar-list" },
  },
  // ── Event Filters ──────────────────────────────────────────────────────
  {
    id: "filters.tentative",
    section: "event-filters",
    label: "Alert for tentative events",
    description: "Fire alerts for meetings you've marked as Maybe.",
    keywords: ["maybe", "tentatively", "rsvp"],
    control: { kind: "toggle", key: "alert_tentative" },
  },
  {
    id: "filters.pending",
    section: "event-filters",
    label: "Alert for unanswered invitations",
    description: "Fire alerts for invitations you haven't responded to yet.",
    keywords: ["pending", "invite", "needs action", "rsvp", "unanswered"],
    control: { kind: "toggle", key: "alert_pending" },
  },
  {
    id: "filters.only-video",
    section: "event-filters",
    label: "Only events with a video link",
    description:
      "Skip alerts for events without a video-conference link (in-person, blocks, reminders).",
    keywords: ["zoom", "meet", "teams", "conference", "call", "video only"],
    control: { kind: "toggle", key: "only_video_events" },
  },
  {
    id: "filters.all-day-tray",
    section: "event-filters",
    label: "Show all-day events in the list",
    description:
      "All-day events never fire alerts; this controls whether they appear in the menu-bar list.",
    keywords: ["allday", "all day", "birthdays", "holidays"],
    control: { kind: "toggle", key: "show_all_day_in_tray" },
  },
  // ── Menu Bar ───────────────────────────────────────────────────────────
  {
    id: "menubar.next-event",
    section: "menu-bar",
    label: "Show next event in the menu bar",
    description: "Display the next meeting's title and countdown beside the icon.",
    keywords: ["title", "countdown", "status bar", "next event"],
    control: { kind: "toggle", key: "show_next_event_in_menu_bar" },
  },
  {
    id: "menubar.title-length",
    section: "menu-bar",
    label: "Title length",
    description: "Truncate the menu-bar event title to this many characters.",
    keywords: ["truncate", "short", "characters", "width"],
    control: { kind: "number", key: "menu_bar_title_chars", min: 4, max: 60, unit: "chars" },
  },
  {
    id: "menubar.tray-icon",
    section: "menu-bar",
    label: "Tray icon",
    description:
      "Menu-bar icon style. Auto adapts to the light/dark menu bar; Light and Dark force a fixed glyph.",
    keywords: ["tray", "icon", "menu bar", "template", "light", "dark", "appearance"],
    control: {
      kind: "select",
      key: "tray_icon",
      options: [
        { value: "auto", label: "Auto (adapts)" },
        { value: "light", label: "Light" },
        { value: "dark", label: "Dark" },
      ],
    },
  },
  // ── Appearance ─────────────────────────────────────────────────────────
  {
    id: "appearance.theme",
    section: "appearance",
    label: "Alert theme",
    description:
      "Visual style for the takeover alert. Use Show Demo Alert to see and hear it on your real displays.",
    keywords: [
      "theme",
      "color",
      "frost",
      "blur",
      "dark",
      "light",
      "sunset",
      "terminal",
      "preview",
      "demo",
    ],
    control: { kind: "theme" },
  },
  // ── Advanced ───────────────────────────────────────────────────────────
  {
    id: "advanced.auto-close",
    section: "advanced",
    label: "Automatically close alerts",
    description:
      "Close an unactioned alert after the timeout below. Off by default: an unattended alert is exactly the one you need to see when you come back.",
    keywords: ["timeout", "dismiss", "auto close", "hide"],
    control: { kind: "toggle", key: "auto_close_enabled" },
  },
  {
    id: "advanced.auto-close-minutes",
    section: "advanced",
    label: "Close alerts after",
    description: "Timeout for automatic closing (when enabled).",
    keywords: ["timeout", "minutes"],
    control: { kind: "number", key: "auto_close_minutes", min: 1, max: 120, unit: "min" },
  },
];

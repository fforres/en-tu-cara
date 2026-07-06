// Settings registry: every setting is DATA — the sidebar TOC,
// the section views, and fuzzy search are all generated from this list.

export type SectionId =
  | "general"
  | "alerts"
  | "calendars"
  | "event-filters"
  | "menu-bar"
  | "appearance"
  | "advanced"
  | "feedback"
  | "about";

export const SECTIONS: Array<{ id: SectionId; label: string }> = [
  { id: "general", label: "General" },
  { id: "alerts", label: "Alerts" },
  { id: "calendars", label: "Calendars" },
  { id: "event-filters", label: "Event Filters" },
  { id: "menu-bar", label: "Menu Bar" },
  { id: "appearance", label: "Appearance" },
  { id: "advanced", label: "Advanced" },
  { id: "feedback", label: "Feedback" },
  { id: "about", label: "About" },
];

/** Mirror of the Rust `Settings` struct (settings.rs). */
export interface Settings {
  enabled_calendar_ids: string[] | null;
  /** Pre-event reminders: minutes before start, 0–3 entries. Empty = only the
   *  mandatory event-start alert fires. */
  reminders: number[];
  alert_sound: string;
  sound_repeat_secs: number;
  /** Default snooze duration (minutes) for "Remind me again". Independent from `reminders`. */
  default_snooze_minutes: number;
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
  /** Anonymized usage telemetry → PostHog. Opt-out; default on. */
  telemetry_enabled: boolean;
  /** Stable random per-install id (telemetry distinct_id). Minted by Rust; not user-facing. */
  device_id: string;
}

export type Control =
  | { kind: "toggle"; key: keyof Settings }
  | { kind: "number"; key: keyof Settings; min: number; max: number; unit: string }
  | { kind: "sound" } // sound picker + preview (alert_sound)
  | { kind: "reminder-list" } // reminders editor (0–3 pre-event reminders)
  | { kind: "calendar-list" } // enabled_calendar_ids editor
  | { kind: "theme" } // theme picker + demo alert button
  | { kind: "select"; key: keyof Settings; options: Array<{ value: string; label: string }> }
  | { kind: "link"; url: string; button: string } // opens a URL in the browser
  | { kind: "feedback" } // suggestion box → PostHog (submit_feedback)
  | { kind: "export-logs" } // save local logs to Downloads + clipboard (export_logs)
  | { kind: "version" } // app version + "Check for Updates"
  | { kind: "changelog" } // full release history (in-code changelog/)
  | { kind: "note" } // description-only (no control)
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
    id: "alerts.reminders",
    section: "alerts",
    label: "Remind me before the event",
    description:
      "Notifications before a meeting starts — add up to three, each at its own time, or remove them all. A notification always fires at the exact start time regardless.",
    keywords: ["lead", "before", "early", "minutes", "reminder", "reminders", "warning", "t-5"],
    control: { kind: "reminder-list" },
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
    id: "alerts.default-snooze",
    section: "alerts",
    label: "Default snooze duration",
    description:
      'How long "Remind me again" postpones an alert. Independent from your reminder times — changing it never touches your reminder schedule.',
    keywords: ["snooze", "remind me again", "delay", "postpone", "later"],
    control: { kind: "number", key: "default_snooze_minutes", min: 1, max: 120, unit: "min" },
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
  {
    id: "advanced.export-logs",
    section: "advanced",
    label: "Export logs",
    description:
      "Save En Tu Cara's local logs to your Downloads folder and copy them to the clipboard, so you can attach them to a bug report. Logs are kept on your Mac and contain no event titles or personal calendar contents.",
    keywords: ["logs", "log", "debug", "diagnostics", "export", "troubleshoot", "support", "bug"],
    control: { kind: "export-logs" },
  },
  {
    id: "advanced.telemetry",
    section: "advanced",
    label: "Share anonymized usage data",
    description:
      "Send anonymized usage telemetry (e.g. whether alerts fired on time, errors) to help improve the app. No event titles, attendees, calendar names, or emails ever leave your Mac — only a random device id and behavioral counts.",
    keywords: ["telemetry", "analytics", "privacy", "posthog", "usage", "tracking", "data"],
    control: { kind: "toggle", key: "telemetry_enabled" },
  },
  // ── Feedback ───────────────────────────────────────────────────────────
  {
    id: "feedback.send",
    section: "feedback",
    label: "Send a suggestion",
    description:
      "Tell us what would make En Tu Cara better — bugs, ideas, anything. Sent anonymously; add your email only if you'd like a reply. Works even if usage data sharing is off.",
    keywords: ["feedback", "suggestion", "idea", "bug", "comment", "contact", "report", "request"],
    control: { kind: "feedback" },
  },
  // ── About ──────────────────────────────────────────────────────────────
  {
    id: "about.version",
    section: "about",
    label: "Version",
    description: "The version you're running.",
    keywords: ["version", "update", "check for updates", "build"],
    control: { kind: "version" },
  },
  {
    id: "about.changelog",
    section: "about",
    label: "What's new",
    description: "Release notes for this and every previous version.",
    keywords: ["changelog", "what's new", "whats new", "release notes", "history", "versions"],
    control: { kind: "changelog" },
  },
  {
    id: "about.project",
    section: "about",
    label: "Open source & volunteer-run",
    description:
      "En Tu Cara is free and open source — use it, fork it, ship it. It's maintained by volunteers in their spare time, so there's no SLA or guaranteed support; please be kind. The MIT license just asks that you keep the attribution.",
    keywords: ["open source", "license", "mit", "volunteer", "free", "attribution"],
    control: { kind: "note" },
  },
  {
    id: "about.source",
    section: "about",
    label: "Source code",
    description: "Browse the project, star it, or open a pull request on GitHub.",
    keywords: ["github", "repo", "source", "code", "repository"],
    control: {
      kind: "link",
      url: "https://github.com/fforres/en-tu-cara",
      button: "View on GitHub",
    },
  },
  {
    id: "about.support",
    section: "about",
    label: "Help & support",
    description:
      "Found a bug or have a request? Open an issue — that's the best way to reach the maintainers.",
    keywords: ["help", "support", "bug", "issue", "report", "feedback", "contact"],
    control: {
      kind: "link",
      url: "https://github.com/fforres/en-tu-cara/issues/new",
      button: "Report an issue",
    },
  },
];

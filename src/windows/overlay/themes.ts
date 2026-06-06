// Overlay themes (user req 2026-06-06): visual styles for backdrop + card +
// accents. Every color is FIXED rgba/hex — never CSS system colors, which
// resolve per-window activation state and broke display uniformity once already
// (see entucara memory: CGWindowList/system-color traps).

export interface Theme {
  id: string;
  label: string;
  /** Tint over the native frost, identical on every display. */
  backdrop: string;
  /** Alert card background. */
  cardBg: string;
  text: string;
  textSecondary: string;
  /** Join button. */
  accent: string;
  accentText: string;
  /** Secondary buttons (Dismiss/Snooze) border. */
  buttonBorder: string;
}

export const THEMES: Theme[] = [
  {
    id: "frost-dark",
    label: "Frost Dark",
    backdrop: "rgba(22, 22, 26, 0.25)",
    cardBg: "rgba(30, 30, 34, 0.92)",
    text: "rgba(255, 255, 255, 0.95)",
    textSecondary: "rgba(255, 255, 255, 0.65)",
    accent: "#3478f6",
    accentText: "#ffffff",
    buttonBorder: "rgba(255, 255, 255, 0.3)",
  },
  {
    id: "frost-light",
    label: "Frost Light",
    backdrop: "rgba(245, 245, 248, 0.35)",
    cardBg: "rgba(255, 255, 255, 0.92)",
    text: "rgba(20, 20, 24, 0.95)",
    textSecondary: "rgba(20, 20, 24, 0.6)",
    accent: "#0a64d8",
    accentText: "#ffffff",
    buttonBorder: "rgba(0, 0, 0, 0.25)",
  },
  {
    id: "sunset",
    label: "Sunset",
    backdrop: "rgba(48, 18, 8, 0.35)",
    cardBg: "rgba(58, 26, 12, 0.94)",
    text: "rgba(255, 244, 235, 0.97)",
    textSecondary: "rgba(255, 214, 180, 0.7)",
    accent: "#ef7d33", // mascot orange
    accentText: "#2a1306",
    buttonBorder: "rgba(255, 200, 160, 0.4)",
  },
  {
    id: "midnight",
    label: "Midnight",
    backdrop: "rgba(6, 10, 24, 0.4)",
    cardBg: "rgba(12, 18, 38, 0.94)",
    text: "rgba(225, 235, 255, 0.96)",
    textSecondary: "rgba(160, 180, 230, 0.7)",
    accent: "#5e8bff",
    accentText: "#0a1020",
    buttonBorder: "rgba(140, 170, 255, 0.35)",
  },
  {
    id: "terminal",
    label: "Terminal",
    backdrop: "rgba(0, 12, 4, 0.45)",
    cardBg: "rgba(2, 20, 8, 0.95)",
    text: "rgba(160, 255, 176, 0.95)",
    textSecondary: "rgba(120, 220, 140, 0.65)",
    accent: "#23d160",
    accentText: "#031007",
    buttonBorder: "rgba(80, 220, 120, 0.4)",
  },
];

export function resolveTheme(id: string | undefined | null): Theme {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

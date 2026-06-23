// Shared style tokens for the settings window. CSS system colors are safe here
// (opaque normal window — the active/inactive system-color trap only applies to
// the overlay panels; see windows/overlay/AGENTS.md).
export const css = {
  hairline: "1px solid color-mix(in srgb, CanvasText 12%, transparent)",
  secondary: "color-mix(in srgb, CanvasText 55%, transparent)",
} as const;

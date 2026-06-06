// Phase 0 stub. Phase 4 builds the real UI against reference-images/tray-example.png:
// ONGOING (color bar, "until 3:00 PM", pie countdown) / UPCOMING (today|all, day groups).
export function TrayPopover() {
  return (
    <main
      style={{
        fontFamily: "-apple-system, BlinkMacSystemFont, sans-serif",
        background: "#1e1e1e",
        color: "#e0e0e0",
        height: "100vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flexDirection: "column",
        gap: 8,
        userSelect: "none",
      }}
    >
      <strong>En Tu Cara</strong>
      <span style={{ opacity: 0.6, fontSize: 13 }}>
        Phase 0 — tray popover stub
      </span>
    </main>
  );
}

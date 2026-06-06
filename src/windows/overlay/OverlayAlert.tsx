import { invoke } from "@tauri-apps/api/core";

// Phase 1b spike content. Phase 5 builds the real alert UI (countdown, Join,
// Snooze, multi-event stacked cards). For the spike the job is purely visual:
// unmissably prove the panel rendered above a fullscreen app on every display.
export function OverlayAlert() {
  return (
    <main
      style={{
        fontFamily: "-apple-system, BlinkMacSystemFont, sans-serif",
        background: "rgba(20, 20, 24, 0.96)",
        color: "#fff",
        height: "100vh",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flexDirection: "column",
        gap: 24,
        userSelect: "none",
      }}
    >
      <div style={{ fontSize: 96 }}>⏰</div>
      <h1 style={{ fontSize: 56, margin: 0 }}>EN TU CARA</h1>
      <p style={{ fontSize: 24, opacity: 0.7, margin: 0 }}>
        Overlay spike — am I above your fullscreen app?
      </p>
      <button
        onClick={() => invoke("close_overlays")}
        style={{
          fontSize: 20,
          padding: "12px 32px",
          borderRadius: 10,
          border: "none",
          background: "#e8833a",
          color: "#fff",
          cursor: "pointer",
        }}
      >
        Dismiss
      </button>
    </main>
  );
}

// First-run onboarding. A real, visible window so a fresh install explains
// itself and asks for the permissions it needs — instead of firing opaque system
// dialogs from a hidden window with no context. Opened on launch by
// tray::maybe_show_onboarding when settings.onboarded is false; "Get started"
// calls finish_onboarding (sets the flag + closes). Styling uses CSS system
// colors so it matches light/dark.
import { useCallback, useEffect, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";

type Status = "checking" | "needed" | "granted" | "denied";

const secondary = "color-mix(in srgb, CanvasText 60%, transparent)";
const card: CSSProperties = {
  display: "flex",
  gap: 12,
  alignItems: "center",
  padding: "12px 14px",
  borderRadius: 10,
  border: "1px solid color-mix(in srgb, CanvasText 14%, transparent)",
};

function PermissionRow({
  icon,
  title,
  desc,
  status,
  actionLabel,
  onAction,
}: {
  icon: string;
  title: string;
  desc: string;
  status: Status;
  actionLabel: string;
  onAction: () => void;
}) {
  return (
    <div style={card}>
      <span style={{ fontSize: 22 }}>{icon}</span>
      <div style={{ flex: 1 }}>
        <div style={{ fontWeight: 600 }}>{title}</div>
        <div style={{ fontSize: 12, color: secondary, marginTop: 2 }}>{desc}</div>
      </div>
      {status === "granted" ? (
        <span style={{ color: "#3fae6b", fontWeight: 600, whiteSpace: "nowrap" }}>✓ Granted</span>
      ) : status === "denied" ? (
        <span style={{ fontSize: 12, color: secondary, maxWidth: 130, textAlign: "right" }}>
          Enable in System Settings
        </span>
      ) : (
        <button
          onClick={onAction}
          disabled={status === "checking"}
          style={{ font: "inherit", padding: "5px 12px", cursor: "pointer", whiteSpace: "nowrap" }}
        >
          {actionLabel}
        </button>
      )}
    </div>
  );
}

function calStatus(s: string): Status {
  if (s === "FullAccess") {
    return "granted";
  }
  if (s === "Denied" || s === "Restricted") {
    return "denied";
  }
  return "needed";
}

export function OnboardingWindow() {
  const [cal, setCal] = useState<Status>("checking");
  const [notif, setNotif] = useState<Status>("checking");
  const [login, setLogin] = useState(true);

  const refreshCal = useCallback(() => {
    invoke<string>("calendar_authorization_status")
      .then((s) => setCal(calStatus(s)))
      .catch(() => setCal("needed"));
  }, []);

  useEffect(() => {
    refreshCal();
    isPermissionGranted()
      .then((g) => setNotif(g ? "granted" : "needed"))
      .catch(() => setNotif("needed"));
    invoke<{ launch_at_login: boolean }>("get_settings")
      .then((s) => setLogin(s.launch_at_login))
      .catch(() => {});
  }, [refreshCal]);

  const grantCal = useCallback(async () => {
    await invoke("request_calendar_access").catch(() => {});
    refreshCal();
  }, [refreshCal]);

  const grantNotif = useCallback(async () => {
    const res = await requestPermission().catch(() => "default");
    setNotif(res === "granted" ? "granted" : "denied");
  }, []);

  const toggleLogin = useCallback((on: boolean) => {
    setLogin(on);
    invoke<Record<string, unknown>>("get_settings")
      .then((s) => invoke("set_settings", { settings: { ...s, launch_at_login: on } }))
      .catch(() => {});
  }, []);

  return (
    <main
      style={{
        font: "13px system-ui",
        colorScheme: "light dark",
        background: "Canvas",
        color: "CanvasText",
        minHeight: "100vh",
        boxSizing: "border-box",
        padding: 24,
        display: "flex",
        flexDirection: "column",
        gap: 18,
      }}
    >
      <div>
        <h1 style={{ fontSize: 22, margin: "0 0 4px" }}>Welcome to En Tu Cara</h1>
        <p style={{ margin: 0, color: secondary }}>
          Unmissable meeting alerts, fully local. Grant a couple of permissions and you're set.
        </p>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <PermissionRow
          icon="📅"
          title="Calendar access"
          desc="Reads your calendar to alert you before meetings. Stays on your Mac."
          status={cal}
          actionLabel="Grant access"
          onAction={() => void grantCal()}
        />
        <PermissionRow
          icon="🔔"
          title="Notifications"
          desc="A heads-up when the app updates itself."
          status={notif}
          actionLabel="Enable"
          onAction={() => void grantNotif()}
        />
        <div style={card}>
          <span style={{ fontSize: 22 }}>🚀</span>
          <div style={{ flex: 1 }}>
            <div style={{ fontWeight: 600 }}>Start at login</div>
            <div style={{ fontSize: 12, color: secondary, marginTop: 2 }}>
              Launch automatically and stay in the menu bar so you never miss a meeting.
            </div>
          </div>
          <input
            type="checkbox"
            role="switch"
            aria-label="Start at login"
            checked={login}
            onChange={(e) => toggleLogin(e.target.checked)}
            style={{ width: 18, height: 18, accentColor: "Highlight" }}
          />
        </div>
      </div>

      <div style={{ marginTop: "auto", display: "flex", justifyContent: "flex-end" }}>
        <button
          onClick={() => void invoke("finish_onboarding")}
          style={{
            font: "inherit",
            fontWeight: 600,
            padding: "8px 18px",
            cursor: "pointer",
            borderRadius: 8,
          }}
        >
          Get started
        </button>
      </div>
    </main>
  );
}

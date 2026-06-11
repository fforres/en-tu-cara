//! TCC grant repair — the self-heal for the POISONED-GRANT state.
//!
//! Incident 2026-06-10: macOS held a legacy-level Calendar record (TCC
//! authValue=2) that calaccessd, requiring the modern full-access level (4),
//! refused to honor — `authorization_status` reported FullAccess while every
//! read returned nothing, ~4.5 min after each launch, across restarts AND
//! re-grants (tccd: "Staged prompting request is invalid: currentAuth: 2
//! desiredAuth: 4"). No store rebuild or re-prompt fixes that; only
//! `tccutil reset Calendar <bundle id>` + a FRESH grant does.
//!
//! Detection lives in `access::GrantRepairTracker` (fed by the scheduler tick);
//! this module owns the destructive act and its safety rails. Two triggers:
//! "auto" (the tracker) and "manual" (the Settings banner's "Repair access").

#![cfg(target_os = "macos")]

/// Cooldown marker for the AUTOMATIC repair — a FILE (mtime = last run), not an
/// in-process static, deliberately: a successful repair prompt RELAUNCHES the
/// app (relaunch-on-grant), so an in-process cooldown resets with it and a
/// machine where the repair doesn't take (the 2026-06-10 wedge: even fresh
/// grants die) would loop reset→prompt→relaunch every few minutes. The file
/// survives restarts; one repair attempt per cooldown per MACHINE, period.
/// Manual (Settings button) bypasses it — an explicit click.
fn cooldown_active() -> bool {
    cooldown_active_at(&crate::paths::data_dir().join("grant-repair-last"))
}

fn stamp_cooldown() {
    stamp_cooldown_at(&crate::paths::data_dir().join("grant-repair-last"));
}

/// Path-parameterised helpers so the cooldown logic is unit-testable without
/// touching the real data dir. The zero-arg wrappers above are the only
/// production call sites.
fn cooldown_active_at(path: &std::path::Path) -> bool {
    const COOLDOWN: std::time::Duration = std::time::Duration::from_secs(6 * 3600);
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|elapsed| elapsed < COOLDOWN)
}

fn stamp_cooldown_at(path: &std::path::Path) {
    let _ = crate::paths::atomic_write(path, b"");
}

/// Destroy + recreate this app's Calendar TCC record. Safety rails, in order:
///   1. Never in test mode (the access machine may be driven by
///      ENTUCARA_TEST_ACCESS, not a real grant).
///   2. Auto runs at most once per 6h (manual bypasses — an explicit click).
///   3. 60s in-flight debounce for ALL triggers: each repair resets the record
///      AND shows a prompt — a re-trigger 3s later (live finding: rapid
///      Settings-button clicks) resets again, invalidating the prompt already
///      on screen, so the user's eventual "Allow" lands on a stale dialog.
///   4. identity::grant_repair_blocker — an ad-hoc build under the PROD bundle
///      id must never destroy the release's grant (gotcha #5).
///
/// On success: rebuild the event store and show the fresh full-access prompt
/// (relaunch-on-grant applies the clean record). Everything off-thread; the
/// caller (scheduler tick or IPC command) never blocks.
pub fn attempt(app: &tauri::AppHandle, trigger: &'static str) {
    if crate::testmode::is_test_mode() {
        return;
    }
    if trigger != "manual" && cooldown_active() {
        log::info!("grant repair: skipped (cooldown; trigger={trigger})");
        return;
    }
    {
        static IN_FLIGHT_SINCE: std::sync::Mutex<Option<std::time::Instant>> =
            std::sync::Mutex::new(None);
        let mut since = IN_FLIGHT_SINCE.lock().unwrap_or_else(|e| e.into_inner());
        let now = std::time::Instant::now();
        if since.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_secs(60)) {
            log::info!("grant repair: skipped (one already in flight; trigger={trigger})");
            return;
        }
        *since = Some(now);
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let bundle_id = app.config().identifier.clone();
        if let Some(blocker) = crate::identity::grant_repair_blocker(&bundle_id) {
            log::warn!("grant repair: SKIPPED — {blocker}");
            crate::telemetry::record(
                "calendar_grant_repair_skipped",
                serde_json::json!({ "trigger": trigger }),
            );
            return;
        }
        log::warn!(
            "grant repair: resetting the Calendar TCC record for {bundle_id} (trigger={trigger}) — \
             status says FullAccess but reads persistently fail (poisoned record); \
             a fresh access prompt follows"
        );
        // Stamp BEFORE acting (and regardless of outcome): the relaunch-on-grant
        // wipes this process, so the file is the only thing standing between a
        // not-taking repair and a prompt loop.
        stamp_cooldown();
        let out =
            std::process::Command::new("tccutil").args(["reset", "Calendar", &bundle_id]).output();
        let ok = out.as_ref().is_ok_and(|o| o.status.success());
        crate::telemetry::record(
            "calendar_grant_repair",
            serde_json::json!({ "trigger": trigger, "ok": ok }),
        );
        if !ok {
            let detail = match out {
                Ok(o) => String::from_utf8_lossy(&o.stderr).trim().to_string(),
                Err(e) => e.to_string(),
            };
            log::warn!("grant repair: tccutil reset failed: {detail}");
            return;
        }
        crate::calendar::invalidate_event_store();
        log::info!("grant repair: TCC record reset — showing the fresh full-access prompt");
        // Relaunch-on-grant applies the clean record process-wide.
        crate::calendar::prompt_access_off_main(app);
    });
}

/// Settings-banner "Repair access" button: the manual entry to the same repair.
/// Returns immediately (all work is off-thread).
#[tauri::command]
pub fn repair_calendar_access(app: tauri::AppHandle) -> Result<(), String> {
    attempt(&app, "manual");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Repair-cooldown regression tests (2026-06-10 incident).
    //
    // The cooldown file is the ONLY guard against a prompt loop on a machine
    // where the repair doesn't take: a successful repair relaunches the app
    // (relaunch-on-grant), wiping all in-process state, so an in-process
    // cooldown would reset with it. The file survives restarts.
    // -----------------------------------------------------------------------

    /// Each test gets a unique scratch directory so parallel test runs can't
    /// step on each other.
    fn cooldown_scratch(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("entucara-cooldown-{tag}-{}", std::process::id()))
    }

    #[test]
    fn cooldown_false_when_no_stamp_file() {
        // No file at all → cooldown is NOT active: the first auto-repair attempt
        // must always be allowed on a machine that has never had a repair.
        let dir = cooldown_scratch("no-stamp");
        let path = dir.join("grant-repair-last");
        // Do NOT create the directory or file.
        assert!(
            !cooldown_active_at(&path),
            "missing stamp file must not be treated as an active cooldown"
        );
    }

    #[test]
    fn cooldown_true_immediately_after_stamp() {
        // A just-written stamp is within any sane 6-hour cooldown window.
        // This is the guard against a repair loop on a machine where the fix
        // doesn't take — the stamp must activate the gate the moment it's written.
        let dir = cooldown_scratch("stamp-active");
        let path = dir.join("grant-repair-last");
        stamp_cooldown_at(&path);
        assert!(cooldown_active_at(&path), "a just-written stamp must make the cooldown active");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cooldown_double_stamp_is_idempotent() {
        // Two stamps in a row must not error and the cooldown must still be active.
        // Idempotency matters: `attempt` stamps BEFORE running tccutil so a
        // process exit mid-repair leaves the file in place. A subsequent
        // auto-trigger before the cooldown expires re-stamps, which is fine.
        let dir = cooldown_scratch("double-stamp");
        let path = dir.join("grant-repair-last");
        stamp_cooldown_at(&path);
        stamp_cooldown_at(&path); // must not panic or corrupt
        assert!(cooldown_active_at(&path), "cooldown must still be active after stamping twice");
        let _ = std::fs::remove_dir_all(dir);
    }
}

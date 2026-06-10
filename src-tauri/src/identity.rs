//! Startup code-signing identity logging — the diagnostic for the lost-access
//! class of bug. macOS keys the calendar (TCC) grant to the app's code-signing
//! identity; if a build runs under a DIFFERENT identity than the one TCC granted
//! (an ad-hoc `tauri dev`/local build vs the Developer-ID release, same bundle
//! id), the grant resets and alerts silently die. Logging the running identity
//! at startup makes an identity change instantly visible in the logs.

/// Parsed `codesign -dvvv` output. The fields we SHIP (identifier + team) are
/// PII-free; the human `authority` line (which can carry a developer name) is
/// kept local-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningInfo {
    pub identifier: String,
    pub team: String,
    pub authority: String,
}

/// The production bundle id (TCC-keyed) and the stable Developer-ID team that
/// must sign it. A build under the prod id signed by anything else is ad-hoc /
/// local — macOS will reset its calendar grant. Dev builds use a separate
/// identifier (see tauri.dev.conf.json) so they never touch the prod grant.
const PROD_IDENTIFIER: &str = "dev.fforres.entucara";
const DEVELOPER_ID_TEAM: &str = "M4M27973Q7";

/// A loud warning when this build will silently lose calendar access: it's
/// running under the PRODUCTION bundle id but isn't signed by the Developer-ID
/// team (so it's ad-hoc/local and TCC will reset the grant). None when it's the
/// real release, or a dev build under its own identifier. Pure + tested.
fn identity_warning(identifier: &str, team: &str) -> Option<String> {
    (identifier == PROD_IDENTIFIER && team != DEVELOPER_ID_TEAM).then(|| {
        format!(
            "this build runs under the production bundle id ({identifier}) but is signed by \
             '{team}', not Developer-ID team {DEVELOPER_ID_TEAM} — macOS will reset its calendar \
             access. For local runs use `pnpm tauri:dev` (separate dev identity)."
        )
    })
}

/// Parse the stderr of `codesign -dvvv`. Pure + tested. Ad-hoc / unsigned
/// binaries (no Authority / TeamIdentifier line) map to clear sentinels so a dev
/// build is obvious in the logs.
pub fn parse_codesign_output(text: &str) -> SigningInfo {
    let value = |prefix: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(prefix).map(|v| v.trim().to_string()))
    };
    let adhoc = text.contains("Signature=adhoc") || text.contains("linker-signed");
    SigningInfo {
        identifier: value("Identifier=").unwrap_or_else(|| "unknown".into()),
        team: value("TeamIdentifier=").unwrap_or_else(|| if adhoc { "adhoc".into() } else { "none".into() }),
        authority: value("Authority=").unwrap_or_else(|| if adhoc { "ad-hoc".into() } else { "unsigned".into() }),
    }
}

/// Shell `codesign` on the running binary and log its identity. Off-thread —
/// `codesign` can take tens of ms and must never touch the main/alarm path
/// (same shell-out discipline as `sw_vers`/`pmset`). The local log keeps the full
/// authority; telemetry ships only the bundle id + team (no developer name).
/// `bundle_id` is the Tauri-configured identifier (= the Info.plist
/// CFBundleIdentifier TCC keys on, and what the `--config` dev override changes)
/// — NOT codesign's signing identifier, which for an ad-hoc build is
/// `<binary>-<cdhash>`, unrelated to the bundle id. The guard keys on bundle_id.
#[cfg(target_os = "macos")]
pub fn log_signing_identity(version: String, bundle_id: String) {
    std::thread::spawn(move || {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let output = std::process::Command::new("codesign")
            .args(["-dvvv", "--verbose=4"])
            .arg(&exe)
            .output();
        let Ok(output) = output else {
            log::warn!("startup identity: codesign failed to run");
            return;
        };
        let info = parse_codesign_output(&String::from_utf8_lossy(&output.stderr));
        log::info!(
            "startup identity: v{version} bundle_id={bundle_id} codesign_id={} team={} authority={:?}",
            info.identifier,
            info.team,
            info.authority
        );
        // Loud guard: an ad-hoc/local build under the prod bundle id WILL reset
        // calendar access (gotcha #5). Surface it (ships via the obs WARN layer).
        if let Some(warning) = identity_warning(&bundle_id, &info.team) {
            log::warn!("{warning}");
        }
        crate::telemetry::record(
            "startup_identity",
            serde_json::json!({
                "version": version,
                "bundle_id": bundle_id,
                "signing_team": info.team,
            }),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_developer_id_signature() {
        // Real shape of `codesign -dvvv` stderr for the notarized release.
        let sample = "Executable=/Applications/En Tu Cara.app/Contents/MacOS/en-tu-cara\n\
            Identifier=dev.fforres.entucara\n\
            CodeDirectory v=20500 flags=0x10000(runtime)\n\
            Signature size=9045\n\
            Authority=Developer ID Application: felipe torres (M4M27973Q7)\n\
            Authority=Developer ID Certification Authority\n\
            TeamIdentifier=M4M27973Q7\n";
        let info = parse_codesign_output(sample);
        assert_eq!(info.identifier, "dev.fforres.entucara");
        assert_eq!(info.team, "M4M27973Q7");
        assert_eq!(info.authority, "Developer ID Application: felipe torres (M4M27973Q7)");
    }

    #[test]
    fn flags_adhoc_builds_clearly() {
        // The dev/local build that collides with the release's TCC grant.
        let sample = "Identifier=dev.fforres.entucara\nSignature=adhoc\nCodeDirectory v=20400\n";
        let info = parse_codesign_output(sample);
        assert_eq!(info.identifier, "dev.fforres.entucara");
        assert_eq!(info.team, "adhoc", "an ad-hoc build must be unmistakable in the logs");
        assert_eq!(info.authority, "ad-hoc");
    }

    #[test]
    fn identity_warning_fires_only_for_adhoc_under_the_prod_id() {
        // The real notarized release → no warning.
        assert_eq!(identity_warning("dev.fforres.entucara", "M4M27973Q7"), None);
        // Ad-hoc/local build under the PROD id → loud warning (it'll lose access).
        assert!(identity_warning("dev.fforres.entucara", "adhoc").is_some());
        assert!(identity_warning("dev.fforres.entucara", "not set").is_some());
        // A dev build under its OWN id → expected, no warning.
        assert_eq!(identity_warning("dev.fforres.entucara.dev", "adhoc"), None);
    }

    #[test]
    fn tolerates_unsigned_or_empty() {
        let info = parse_codesign_output("");
        assert_eq!(info.identifier, "unknown");
        assert_eq!(info.team, "none");
        assert_eq!(info.authority, "unsigned");
    }
}

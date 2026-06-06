# Releasing & self-update

How a new version of En Tu Cara gets built, published, and how installed copies
update themselves.

## TL;DR — cut a release

From a clean, up-to-date `main`:

```sh
pnpm release patch     # 0.1.0 -> 0.1.1   (bugfixes)
pnpm release minor     # 0.1.0 -> 0.2.0   (features)
pnpm release major     # 0.1.0 -> 1.0.0   (breaking)
# --patch / --minor / --major also work; add --dry-run to preview.
```

That's it. The rest is automated.

## What the pipeline does

```
pnpm release minor
  │  (local — scripts/release.mjs)
  ├─ guards: on main, clean tree, in sync with origin
  ├─ bumps version in the 3 files that must agree + Cargo.lock:
  │     package.json · src-tauri/tauri.conf.json · src-tauri/Cargo.toml
  ├─ git commit  "chore(release): v0.2.0"
  ├─ git tag     v0.2.0
  └─ git push --follow-tags
         │
         ▼  (tag push triggers CI)
.github/workflows/release.yml  on  macos-14
  ├─ tauri-apps/tauri-action builds  --target universal-apple-darwin
  ├─ signs the updater artifact with TAURI_SIGNING_PRIVATE_KEY (minisign)
  ├─ creates the GitHub Release  "En Tu Cara v0.2.0"
  └─ uploads:  En Tu Cara_0.2.0_universal.dmg
               En Tu Cara.app.tar.gz  (+ .sig)
               latest.json            ← the updater manifest
         │
         ▼  (within seconds of next launch, on every installed copy)
in-app updater  (src/lib/updater.ts → tauri-plugin-updater)
  ├─ fetches  releases/latest/download/latest.json
  ├─ sees 0.2.0 > installed version
  ├─ downloads the .tar.gz, verifies its .sig against the embedded pubkey
  ├─ swaps the .app in place
  └─ relaunches into the new version
```

The version bump is **tag-driven, not merge-driven** — this is the standard
Tauri convention. Merging to `main` does _not_ publish anything; you decide when
to ship by running `pnpm release`. (If you ever want merge-to-main auto-releases,
add `release-please` or `changesets` on top — but that's an extra layer, not
needed here.)

## Self-update, long term

Installed apps stay current on their own:

- **Endpoint** (`tauri.conf.json` → `plugins.updater.endpoints`) points at
  `releases/latest/download/latest.json` — a stable URL that always resolves to
  the newest release. You never touch it again.
- **Check timing**: `src/main.tsx` calls `runStartupUpdateCheck()` ~8s after the
  tray window boots (release builds only). Change the cadence there — e.g. also
  check on an interval, or add a "Check for Updates…" tray menu item that calls
  `checkForUpdate()` from `src/lib/updater.ts`.
- **Trust**: each bundle is signed with the minisign key generated at setup; the
  app refuses any update whose signature doesn't match the `pubkey` baked into
  `tauri.conf.json`. Lose the private key and you can't ship updates the existing
  installs will accept — see "Keys" below.

### macOS signing reality (read this)

The updater **mechanism** works whether or not the app is Apple-notarized. But
Gatekeeper does not:

- **Today (ad-hoc signed):** the `.dmg` download shows the "unidentified
  developer / can't be opened" warning on first launch, and a self-update that
  relaunches into an un-notarized bundle can hit the same wall. That's why
  `runStartupUpdateCheck()` installs + relaunches automatically — flip it to
  notify-only (`runStartupUpdateCheck(false)`) until notarization is set up if
  the relaunch proves disruptive.
- **Notarized (recommended once you have an Apple Developer account, $99/yr):**
  no warnings, seamless self-update. Add these six repo secrets and the workflow
  uses them automatically (already wired in `release.yml`):

  | Secret                       | What                                           |
  | ---------------------------- | ---------------------------------------------- |
  | `APPLE_CERTIFICATE`          | base64 of your Developer ID `.p12`             |
  | `APPLE_CERTIFICATE_PASSWORD` | the `.p12` password                            |
  | `APPLE_SIGNING_IDENTITY`     | e.g. `Developer ID Application: Name (TEAMID)` |
  | `APPLE_ID`                   | your Apple ID email                            |
  | `APPLE_PASSWORD`             | an app-specific password                       |
  | `APPLE_TEAM_ID`              | your 10-char team id                           |

  Set with `gh secret set APPLE_CERTIFICATE < cert.b64`, etc.

## Keys & secrets

The updater signing keypair was generated with:

```sh
pnpm tauri signer generate -w ~/.tauri/en-tu-cara-updater.key
```

- **Public key** → committed in `tauri.conf.json` (`plugins.updater.pubkey`).
- **Private key** → `~/.tauri/en-tu-cara-updater.key` (empty password) and stored
  as repo secrets `TAURI_SIGNING_PRIVATE_KEY` /
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. **Back this file up** somewhere safe
  (password manager). If lost, existing installs will reject every future
  update — you'd have to ship a new pubkey via a manually-downloaded build.

## Adding Windows / Linux later

This app is macOS-only today (`bundle.targets`, NSPanel overlay, EventKit). To
ship other platforms, turn `release.yml` into a build matrix (one runner per OS)
— see the official template:
https://v2.tauri.app/distribute/pipelines/github/ . `latest.json` already carries
per-platform entries, so the updater handles the rest.

## References

- tauri-action: https://github.com/tauri-apps/tauri-action
- Tauri updater plugin: https://v2.tauri.app/plugin/updater/
- GitHub pipeline guide: https://v2.tauri.app/distribute/pipelines/github/

# Releasing & self-update

How a new version of En Tu Cara gets built, published, and how installed copies
update themselves.

## The model: release.json is the trigger

`release.json` holds the single source-of-truth version. **A release is cut when
that version changes on `main` — nothing else.** Ordinary merges (that don't
touch `release.json`) never publish anything, and CI never guesses a version
number: you write it. The diff between two version tags is exactly "what shipped
in this release," and `release.json`'s `notes` field becomes the release body.

## TL;DR — cut a release

```sh
pnpm release patch     # 0.1.0 -> 0.1.1   (bugfixes)
pnpm release minor     # 0.1.0 -> 0.2.0   (features)
pnpm release major     # 0.1.0 -> 1.0.0   (breaking)
# --dry-run to preview; --commit to also make the bump commit.
```

Then open a PR with that bump and merge it to `main`. That's the release.

## What the pipeline does

```
pnpm release minor                       (local — scripts/release.mjs)
  ├─ reads the current version from release.json
  └─ writes the new version to ALL of:
        release.json (source) · package.json · tauri.conf.json
        · Cargo.toml · Cargo.lock
  → you review the diff, commit, PR, and merge to main
         │
         ▼  (push to main touching release.json triggers CI)
.github/workflows/cut-release.yml  (gate)
  ├─ checks the build files agree with release.json (else fails loudly)
  ├─ if a tag vX.Y.Z already exists → stop (idempotent, no double release)
  ├─ otherwise: create + push tag vX.Y.Z
  └─ invoke ↓
.github/workflows/release.yml  on  macos-14   (build, reusable)
  ├─ tauri-apps/tauri-action builds  --target universal-apple-darwin
  ├─ signs the updater artifact with TAURI_SIGNING_PRIVATE_KEY (minisign)
  ├─ creates the GitHub Release (body = release.json notes)
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

Why this shape:

- **No auto-increment guessing.** With many branches merging to `main`, "the next
  patch" is ambiguous — so CI doesn't decide it. You bump `release.json` when (and
  to whatever) you intend, in a reviewable PR.
- **The bump number isn't tied to the branch.** patch/minor/major is just how you
  choose the number locally; merging the change to `main` is what ships it.
- **Manual tag escape hatch.** Pushing a `vX.Y.Z` tag by hand also builds (the
  `release.yml` tag trigger), for the rare case you want to release a specific
  commit without touching `release.json`.

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
  no warnings, seamless self-update. The `APPLE_*` env vars are intentionally
  **not** wired in `release.yml` right now — passing them empty makes
  tauri-action fail the bundle at `security import`. To enable notarization,
  uncomment/add the six `APPLE_*` lines back to the tauri-action `env:` block
  **and** set the matching repo secrets:

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

## Key rotation (seamless, zero manual reinstalls)

Rotating the signing key normally breaks auto-update for existing installs: they
trust only the **public key compiled into them**, so a release signed with a new
key fails their signature check and they freeze until someone reinstalls by hand.
There is exactly one way to avoid that — and it only works **while you still hold
the old key** (a lost key has no seamless path).

The trick: a bundle's **signature** and the **pubkey baked inside it** are
independent. So you can ship one **bridge release** that is _signed with the old
key_ (existing installs accept it) but _carries the new pubkey inside_ (so they
trust the new key from then on).

```
Precondition: you STILL possess the old private key.

1. Generate the new pair, back it up to 1Password immediately:
     pnpm tauri signer generate -w ~/.tauri/en-tu-cara-updater-v2.key

2. Put the NEW pubkey in tauri.conf.json (plugins.updater.pubkey). Commit it.

3. Leave the GitHub secret TAURI_SIGNING_PRIVATE_KEY = OLD key. Do NOT change it.

4. Ship the bridge release (pnpm release minor, or merge to main):
     CI signs with the OLD key  → existing installs verify & accept it
     the build embeds the NEW pubkey → they now trust the new key going forward

5. WAIT until every install has taken the bridge release (for this app that's
   basically your own machines; for a wider base, have the app report its
   version somewhere so you know when the base has moved).

6. Only NOW swap the signing secret to the new key:
     gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/en-tu-cara-updater-v2.key
     gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --body ''   # if empty

7. Every release after this is signed with the NEW key. Installs that took the
   bridge accept it seamlessly. Nobody reinstalled anything. Keep the old key
   archived until you're certain no install predates the bridge.
```

Critical ordering: the **pubkey flips at step 2 (the build)**, but the **signing
secret flips only at step 6 — after the bridge has propagated**. Flip the signing
key too early and existing installs reject it; you're back to manual reinstalls.

What does _not_ help (don't go down these): Tauri allows multiple updater
**endpoints** (mirrors/fallback) but only **one `pubkey`** — there is no "trust
old OR new key." That single-key constraint is why the bridge release is the only
seamless mechanism.

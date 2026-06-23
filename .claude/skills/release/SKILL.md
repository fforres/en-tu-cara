---
name: release
description: Cut a new En Tu Cara release — bump the version (patch/minor/major) and write descriptive, user-facing release notes from everything that landed since the last published release. Use when the user says "bump the version", "release minor/patch/major", "cut a release", or "prepare release notes". Wraps scripts/release.mjs and writes release.json's notes.
---

# Release

Bump the app version and write the release notes for it. This project already has
the mechanical bump in `scripts/release.mjs` (`pnpm release <kind>`), which is the
single source of truth — this skill's value-add is **figuring out what actually
shipped since the last release and turning it into good notes**, then driving the
bump. Read `docs/RELEASING.md` once if anything here is unclear.

## The model (don't fight it)

- `release.json` holds the version + the `notes` body. A release is cut **only**
  when that version changes on `main` (CI: `.github/workflows/cut-release.yml`).
- `pnpm release <kind>` writes the new version to `release.json` AND the 4 files
  the build reads (`package.json`, `src-tauri/tauri.conf.json`,
  `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`) so they can't drift. It also
  **clears** `release.json.notes` (so stale notes can't ship) — you must write
  fresh notes after bumping.
- This skill does NOT push, tag, or merge. It leaves a reviewed bump + notes for
  the user to commit → PR → merge.

## Inputs

The bump kind comes from the user's words; map to semver:
- **patch** — bugfixes only (`0.8.0 → 0.8.1`)
- **minor** — new features, backwards compatible (`0.8.0 → 0.9.0`)
- **major** — breaking changes (`0.8.0 → 1.0.0`)

If the user didn't say which, infer it from the changes (features → minor, fixes
only → patch) and state your choice; ask only if genuinely ambiguous.

## Steps

1. **Find the previous published release** (its tag + commit), so you know the
   range of "what's new":
   ```sh
   gh release view --json tagName,targetCommitish,publishedAt
   ```
   If `gh` is unavailable or unauthenticated, fall back to the API:
   ```sh
   curl -fsSL https://api.github.com/repos/fforres/en-tu-cara/releases/latest \
     | grep -E '"tag_name"|"target_commitish"'
   ```
   Resolve the tag to a commit: `git rev-list -n1 <tag>`.

2. **Gather what shipped** between that commit and `HEAD` — read the actual
   changes, don't guess:
   ```sh
   git log <prev-tag>..HEAD --oneline
   git log <prev-tag>..HEAD --stat        # see which areas changed
   ```
   Pull richer context from merged PRs in the range when useful:
   ```sh
   gh pr list --state merged --base main --limit 30 \
     --json number,title,mergedAt,body
   ```
   Read the diffs/PR bodies for anything non-obvious. Group changes by
   user-visible theme, not by commit.

3. **Bump the version.** Preview first, then write:
   ```sh
   pnpm release <kind> --dry-run     # confirm the old → new number
   pnpm release <kind>               # writes the 5 files, clears notes
   ```
   Do NOT pass `--commit` — we write notes before committing.

4. **Write the release notes** into `release.json`'s `notes` field. Match the
   established voice (see "Notes style" below). The new version number is already
   in `release.json` after step 3 — read it and open the body with it.

5. **Verify nothing drifted**, then hand off:
   ```sh
   git diff --stat        # should be exactly the 5 bump files + release.json notes
   pnpm test && cargo test --manifest-path src-tauri/Cargo.toml   # sanity
   ```
   Tell the user the bump is staged in the working tree and the notes are written,
   and that the release is cut by: commit `chore(release): vX.Y.Z` → PR → merge to
   `main`. Offer to commit it (don't push unless asked).

## Notes style (match the shipped releases)

Read the previous release body for the exact tone:
```sh
gh release view <prev-tag> --json body -q .body
```
The house style is **user-facing prose, not a changelog**:
- First line: `En Tu Cara X.Y.Z — <one-line theme of the release> (macOS, Apple Silicon).`
- A short paragraph on the "why" when there's a story (an incident, a papercut).
- A few themed groups with `-` bullets describing the *behavior the user gets*,
  not the implementation. No commit hashes, no file names, no internal module
  names. Write what changes for someone using the app.
- Keep it honest: if something is a known limitation, say so plainly.

`release.json.notes` is a single JSON string — embed newlines as `\n`. Easiest is
to write the prose to a temp file and set it with a tiny script, or edit
`release.json` directly and double-check it stays valid JSON
(`node -e "JSON.parse(require('fs').readFileSync('release.json'))"`).

## Guardrails

- Never bump `release.json` by hand — always go through `pnpm release <kind>` so
  all 5 files stay in lockstep (the CI gate fails loudly if they disagree).
- Never push or create the tag — merging the version change to `main` is what
  cuts the release; that's the human's call.
- Empty notes publish a generic body — always write notes after bumping.
- If `release.json.notes` was non-empty before the bump, `release.mjs` warns it
  cleared them; that's expected — you're replacing them anyway.

---
name: release
description: Cut a release — bump the version (patch/minor/major) and write user-facing release notes for everything since the last release. Use for "bump the version", "release minor/patch/major", "cut a release". Wraps scripts/release.mjs + release.json.
---

# Release

`scripts/release.mjs` does the mechanical bump; your job is to figure out what
shipped and write the notes. See `docs/RELEASING.md` for how a release is cut.

1. Bump kind from the user (patch=fixes, minor=features, major=breaking); infer
   from the changes if unsaid.
2. Find the last release: `gh release view --json tagName` (fallback:
   `curl -fsSL https://api.github.com/repos/fforres/en-tu-cara/releases/latest`).
3. Read what shipped since: `git log <tag>..HEAD --stat`, plus
   `gh pr list --state merged --base main --json number,title,body`.
4. `pnpm release <kind>` — writes the 5 version files, clears `release.json`
   notes. No `--commit`.
5. Write user-facing notes into `release.json`'s `notes` (JSON string, `\n` for
   newlines); match the prior body's voice: `gh release view <tag> -q .body`.
6. Verify (`git diff --stat`, tests), then commit `chore(release): vX.Y.Z`. Don't
   push or tag — merging the version change to `main` is what cuts the release.

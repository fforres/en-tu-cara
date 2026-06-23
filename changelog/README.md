# changelog/

The in-code history of released versions — one file per version, `vX.Y.Z.md`.
These are the source for the in-app **What's New** viewer (Settings → About) and,
later, the public website.

## Format

```markdown
---
version: 0.9.0
date: 2026-06-23
title: One alert per meeting, even across accounts
---

Markdown release notes. Paragraphs, `## headings`, `- bullet lists`, and
**bold** render in-app (see src/lib/markdown.tsx).
```

- `version` — plain semver `X.Y.Z`, matching the filename (`v` + version + `.md`).
- `date` — release date, `YYYY-MM-DD` (UTC).
- `title` — short human headline (the GitHub release names are just "En Tu Cara vX.Y.Z").
- Body — the user-facing notes, kept verbatim from what shipped.

The app loads these via `import.meta.glob` in `src/lib/changelog.ts`, sorted
newest-first.

## When you cut a release

Releasing is still driven by `release.json` (see `docs/RELEASING.md`). The
changelog is maintained alongside it — for each new version, add
`changelog/v<version>.md` with the same notes you put in `release.json`'s `notes`
field. (They're kept in sync by hand; `release.json.notes` is what CI publishes,
this file is what the app and website show.)

// The in-code release history. One `changelog/v<version>.md` file per shipped
// version (frontmatter + markdown body); this module loads them at build time
// via import.meta.glob and exposes them newest-first to the "What's New" viewer
// (src/windows/settings) and the self-update UI. See changelog/README.md.

export interface ReleaseEntry {
  /** Plain semver, e.g. "0.9.0". */
  version: string;
  /** Release date, "YYYY-MM-DD" (UTC). */
  date: string;
  /** Short human headline. */
  title: string;
  /** User-facing notes, markdown. */
  body: string;
}

/**
 * Parse one `changelog/*.md` file (frontmatter + body). Pure + exported so it
 * can be unit-tested without the glob. Frontmatter keys are split on the first
 * colon, so values may contain colons.
 */
export function parseRelease(raw: string): ReleaseEntry {
  const text = raw.replace(/\r\n/g, "\n");
  const m = /^---\n([\s\S]*?)\n---\n?/.exec(text);
  if (!m) {
    throw new Error("changelog entry is missing its `---` frontmatter block");
  }
  const fm: Record<string, string> = {};
  for (const line of m[1].split("\n")) {
    const i = line.indexOf(":");
    if (i === -1) {
      continue;
    }
    fm[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  if (!fm.version) {
    throw new Error("changelog entry frontmatter is missing `version`");
  }
  return {
    version: fm.version,
    date: fm.date ?? "",
    title: fm.title ?? "",
    body: text.slice(m[0].length).trim(),
  };
}

/** Compare two semver strings so the newer one sorts first. */
export function compareVersionsDesc(a: string, b: string): number {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const diff = (pb[i] || 0) - (pa[i] || 0);
    if (diff !== 0) {
      return diff;
    }
  }
  return 0;
}

// Every shipped version's notes, baked into the bundle. `v*.md` excludes the
// directory README. Sorted newest-first.
const files = import.meta.glob<string>("/changelog/v*.md", {
  query: "?raw",
  import: "default",
  eager: true,
});

export const releases: ReleaseEntry[] = Object.values(files)
  .map((raw) => parseRelease(raw))
  .sort((a, b) => compareVersionsDesc(a.version, b.version));

/** The release entry for a given version, if it's in the bundled history. */
export function findRelease(version: string): ReleaseEntry | undefined {
  return releases.find((r) => r.version === version);
}

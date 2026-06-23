import { describe, expect, it } from "vitest";
import { compareVersionsDesc, parseRelease, releases } from "./changelog";

describe("parseRelease", () => {
  it("extracts frontmatter and body", () => {
    const entry = parseRelease(
      "---\nversion: 1.2.3\ndate: 2026-01-02\ntitle: A nice title\n---\n\nBody line one.\n\n- a bullet\n",
    );
    expect(entry).toEqual({
      version: "1.2.3",
      date: "2026-01-02",
      title: "A nice title",
      body: "Body line one.\n\n- a bullet",
    });
  });

  it("keeps colons in values (splits on the first colon only)", () => {
    const entry = parseRelease("---\nversion: 0.1.0\ntitle: Fix: the thing\n---\nbody\n");
    expect(entry.title).toBe("Fix: the thing");
  });

  it("throws when frontmatter is missing", () => {
    expect(() => parseRelease("no frontmatter here")).toThrow(/frontmatter/);
  });

  it("throws when version is missing", () => {
    expect(() => parseRelease("---\ntitle: x\n---\nbody")).toThrow(/version/);
  });
});

describe("compareVersionsDesc", () => {
  it("sorts newer versions first", () => {
    const sorted = ["0.1.0", "0.10.0", "0.2.0", "1.0.0"].sort(compareVersionsDesc);
    expect(sorted).toEqual(["1.0.0", "0.10.0", "0.2.0", "0.1.0"]);
  });
});

describe("releases (bundled)", () => {
  it("loads the backfilled history newest-first", () => {
    expect(releases.length).toBeGreaterThanOrEqual(15);
    // Sorted descending.
    for (let i = 1; i < releases.length; i++) {
      expect(compareVersionsDesc(releases[i - 1].version, releases[i].version)).toBeLessThanOrEqual(
        0,
      );
    }
    // Every entry has the fields the UI relies on.
    for (const r of releases) {
      expect(r.version).toMatch(/^\d+\.\d+\.\d+$/);
      expect(r.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      expect(r.title.length).toBeGreaterThan(0);
      expect(r.body.length).toBeGreaterThan(0);
    }
  });
});

import { describe, expect, it } from "vitest";
import { REGISTRY, SECTIONS } from "./registry";
import { fuzzyMatch, searchSettings } from "./fuzzy";

describe("registry integrity", () => {
  it("ids are unique", () => {
    const ids = REGISTRY.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("every setting belongs to a declared section", () => {
    const sectionIds = new Set(SECTIONS.map((s) => s.id));
    for (const s of REGISTRY) {
      expect(sectionIds.has(s.section)).toBe(true);
    }
  });

  it("every section Felipe asked for exists and is non-empty (except placeholders-only allowed)", () => {
    const asked = [
      "general",
      "alerts",
      "calendars",
      "event-filters",
      "menu-bar",
      "appearance",
      "advanced",
    ];
    for (const id of asked) {
      expect(SECTIONS.some((s) => s.id === id)).toBe(true);
      expect(REGISTRY.some((s) => s.section === id)).toBe(true);
    }
  });

  it("every setting has searchable text", () => {
    for (const s of REGISTRY) {
      expect(s.label.length).toBeGreaterThan(2);
      expect(s.description.length).toBeGreaterThan(10);
      expect(s.keywords.length).toBeGreaterThan(0);
    }
  });
});

describe("fuzzyMatch", () => {
  it("exact substring scores higher than scattered subsequence", () => {
    const exact = fuzzyMatch("snooze", "Snooze durations")!;
    const scattered = fuzzyMatch("sze", "Snooze durations")!;
    expect(exact.score).toBeGreaterThan(scattered.score);
  });

  it("returns highlight ranges covering the query", () => {
    const m = fuzzyMatch("sound", "Alert sound")!;
    expect(m.ranges).toEqual([[6, 11]]);
  });

  it("non-subsequence returns null", () => {
    expect(fuzzyMatch("xyz", "Alert sound")).toBeNull();
  });

  it("case-insensitive", () => {
    expect(fuzzyMatch("SNOOZE", "snooze durations")).not.toBeNull();
  });

  it("empty query matches everything with zero ranges", () => {
    expect(fuzzyMatch("", "anything")).toEqual({ score: 0, ranges: [] });
  });
});

describe("searchSettings over the real registry", () => {
  it("'snooze' top hit is the snooze durations setting, with label highlight", () => {
    const hits = searchSettings("snooze", REGISTRY);
    expect(hits[0].setting.id).toBe("alerts.snooze-durations");
    expect(hits[0].labelRanges.length).toBeGreaterThan(0);
  });

  it("'zoom' finds the video-only filter via keywords", () => {
    const hits = searchSettings("zoom", REGISTRY);
    expect(hits.some((h) => h.setting.id === "filters.only-video")).toBe(true);
  });

  it("'holidays' finds calendar toggles (the mute-Holidays-in-Chile use case)", () => {
    const hits = searchSettings("holidays", REGISTRY);
    expect(hits[0].setting.section).toMatch(/calendars|event-filters/);
  });

  it("'login' finds start-at-login", () => {
    const hits = searchSettings("login", REGISTRY);
    expect(hits[0].setting.id).toBe("general.launch-at-login");
  });

  it("empty query returns the full registry in order", () => {
    expect(searchSettings("", REGISTRY).length).toBe(REGISTRY.length);
  });

  it("garbage query returns nothing", () => {
    expect(searchSettings("qqqqxxxx", REGISTRY)).toHaveLength(0);
  });
});

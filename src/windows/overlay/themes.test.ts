import { describe, expect, it } from "vitest";
import { resolveTheme, THEMES } from "./themes";

describe("themes registry", () => {
  it("ids unique and complete fields", () => {
    const ids = THEMES.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const t of THEMES) {
      for (const v of Object.values(t)) {
        expect(String(v).length).toBeGreaterThan(0);
      }
    }
  });

  it("includes the approved default frost-dark first", () => {
    expect(THEMES[0].id).toBe("frost-dark");
  });

  it("resolveTheme falls back to default on unknown/null", () => {
    expect(resolveTheme("nope").id).toBe("frost-dark");
    expect(resolveTheme(null).id).toBe("frost-dark");
    expect(resolveTheme("terminal").id).toBe("terminal");
  });

  it("no theme uses CSS system colors (the activation-state trap)", () => {
    for (const t of THEMES) {
      for (const v of Object.values(t)) {
        expect(String(v)).not.toMatch(/Canvas|Highlight|GrayText|Field/);
      }
    }
  });
});

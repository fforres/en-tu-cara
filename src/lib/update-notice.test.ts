import { describe, expect, it } from "vitest";
import { updatedNotice } from "./update-notice";

describe("updatedNotice", () => {
  it("returns a notice when the version changed since last launch", () => {
    const notice = updatedNotice("0.1.0", "0.1.1");
    expect(notice).toEqual({
      title: "En Tu Cara updated",
      body: "Now running v0.1.1 (was v0.1.0).",
    });
  });

  it("is null on a fresh install (no recorded prior version)", () => {
    expect(updatedNotice(null, "0.1.0")).toBeNull();
    expect(updatedNotice(undefined, "0.1.0")).toBeNull();
    expect(updatedNotice("", "0.1.0")).toBeNull();
  });

  it("is null when the version is unchanged", () => {
    expect(updatedNotice("0.2.0", "0.2.0")).toBeNull();
  });

  it("works across minor and major jumps", () => {
    expect(updatedNotice("0.1.5", "0.2.0")?.body).toBe("Now running v0.2.0 (was v0.1.5).");
    expect(updatedNotice("0.9.9", "1.0.0")?.body).toBe("Now running v1.0.0 (was v0.9.9).");
  });
});

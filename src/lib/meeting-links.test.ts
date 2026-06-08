import { describe, expect, it } from "vitest";
import { extractMeetingLink, isWebUrl } from "./meeting-links";

// Fixture matrix: each provider × {url-field, location, notes} +
// adversarial cases. Bodies modeled on real invite formats.

const zoom = "https://us04web.zoom.us/j/4121166431?pwd=S2J2Yy9mY3ZOdz09";
const meet = "https://meet.google.com/abc-defg-hij";
const teams =
  "https://teams.microsoft.com/l/meetup-join/19%3ameeting_NzA2%40thread.v2/0?context=%7b%22Tid%22%3a%22x%22%7d";
const webex = "https://skyward.webex.com/meet/felipe";
const jitsi = "https://meet.jit.si/SkywardStandup";
const whereby = "https://whereby.com/skyward-room";
const around = "https://meet.around.co/r/abcdef";
const discord = "https://discord.gg/skyward";

describe("extractMeetingLink — provider × field matrix", () => {
  const providers: Array<[string, string, string]> = [
    ["zoom", zoom, "zoom"],
    ["meet", meet, "meet"],
    ["teams", teams, "teams"],
    ["webex", webex, "webex"],
    ["jitsi", jitsi, "jitsi"],
    ["whereby", whereby, "whereby"],
    ["around", around, "around"],
    ["discord", discord, "discord"],
  ];

  for (const [name, url, provider] of providers) {
    it(`${name}: url field`, () => {
      const r = extractMeetingLink({ url });
      expect(r).toMatchObject({ provider, source: "url" });
    });
    it(`${name}: location field`, () => {
      const r = extractMeetingLink({ location: `Room 4 — ${url}` });
      expect(r).toMatchObject({ provider, source: "location" });
    });
    it(`${name}: notes field (prose)`, () => {
      const r = extractMeetingLink({
        notes: `Felipe is inviting you.\n\nJoin: ${url}\n\nAgenda follows.`,
      });
      expect(r).toMatchObject({ provider, source: "notes" });
    });
  }
});

describe("extractMeetingLink — adversarial cases", () => {
  it("url field wins over location and notes", () => {
    const r = extractMeetingLink({ url: meet, location: zoom, notes: teams });
    expect(r).toMatchObject({ provider: "meet", source: "url" });
  });

  it("location wins over notes", () => {
    const r = extractMeetingLink({ location: zoom, notes: meet });
    expect(r).toMatchObject({ provider: "zoom", source: "location" });
  });

  it("HTML notes (Google invite blob)", () => {
    const r = extractMeetingLink({
      notes: `<a href="${zoom}">Join Zoom Meeting</a><br><b>Meeting ID:</b> 412 116 6431`,
    });
    expect(r?.provider).toBe("zoom");
    expect(r?.url).toBe(zoom); // no trailing "> dragged in
  });

  it("trailing punctuation stripped", () => {
    const r = extractMeetingLink({ notes: `Join here: ${meet}.` });
    expect(r?.url).toBe(meet);
  });

  it("zoomgov + vanity subdomain", () => {
    const r = extractMeetingLink({ location: "https://company.zoomgov.com/j/123456" });
    expect(r?.provider).toBe("zoom");
  });

  it("generic fallback: self-hosted meet domain (tray reference: meet.bman.dev)", () => {
    const r = extractMeetingLink({ notes: "https://meet.bman.dev/skyward-sync" });
    expect(r).toMatchObject({ provider: "generic" });
  });

  it("plain-prose URL-less event → null", () => {
    expect(extractMeetingLink({ notes: "Lunch with Marta", location: "Café Pinares" })).toBeNull();
  });

  it("non-meeting URL in notes → null (no false positives on docs links)", () => {
    expect(extractMeetingLink({ notes: "Doc: https://docs.google.com/document/d/abc" })).toBeNull();
  });

  it("teams.live.com consumer links", () => {
    const r = extractMeetingLink({ url: "https://teams.live.com/meet/9351234567890" });
    expect(r?.provider).toBe("teams");
  });

  it("zoom link with /s/ (sso) path", () => {
    const r = extractMeetingLink({ url: "https://corp.zoom.us/s/987654?pwd=tok" });
    expect(r?.provider).toBe("zoom");
  });

  it("empty/null fields tolerated", () => {
    expect(extractMeetingLink({})).toBeNull();
    expect(extractMeetingLink({ url: null, location: null, notes: null })).toBeNull();
  });
});

describe("isWebUrl — opener scheme guard", () => {
  it("accepts https", () => {
    expect(isWebUrl("https://x")).toBe(true);
  });
  it("accepts http", () => {
    expect(isWebUrl("http://x")).toBe(true);
  });
  it("rejects javascript: scheme", () => {
    expect(isWebUrl("javascript:alert(1)")).toBe(false);
  });
  it("rejects file: scheme", () => {
    expect(isWebUrl("file:///etc")).toBe(false);
  });
  it("rejects non-URL garbage", () => {
    expect(isWebUrl("not a url")).toBe(false);
  });
});

import { describe, expect, it } from "vitest";
import { accountsForEvent, type AccountInfo, type CalendarRef } from "./accounts";

const lookup = new Map<string, AccountInfo>([
  ["felipe-primary", { account: "felipe@skyward.ai" }],
  ["israel", { account: "felipe@skyward.ai" }], // subscribed colleague — SAME account
  ["google-gmail", { account: "Google" }],
  ["skyward-gmail", { account: "felipe@skyward.ai" }],
  ["jsconf-gmail", { account: "felipe@jsconf.cl" }],
  ["birthdays", { account: null }], // local calendar, no account
]);

describe("accountsForEvent", () => {
  it("collapses colleague-calendar duplicates that live under one account", () => {
    // ENGINEERING Sync: on the user's own calendar AND a subscribed colleague's
    // (whose calendar title is their email) — but BOTH are the felipe@skyward.ai
    // account, so this is ONE origin, not two.
    const calendars: CalendarRef[] = [
      { calendar_id: "felipe-primary", calendar_title: "felipe@skyward.ai" },
      { calendar_id: "israel", calendar_title: "israel@skyward.ai" },
    ];
    expect(accountsForEvent(calendars, lookup)).toEqual(["felipe@skyward.ai"]);
  });

  it("lists every distinct account for a genuine cross-account duplicate", () => {
    // Sacar la basura: the same "FELIPE TORRES — GMAIL" calendar reachable from
    // three synced accounts → three origins.
    const calendars: CalendarRef[] = [
      { calendar_id: "google-gmail", calendar_title: "FELIPE TORRES — GMAIL" },
      { calendar_id: "skyward-gmail", calendar_title: "FELIPE TORRES — GMAIL" },
      { calendar_id: "jsconf-gmail", calendar_title: "FELIPE TORRES — GMAIL" },
    ];
    expect(accountsForEvent(calendars, lookup)).toEqual([
      "Google",
      "felipe@skyward.ai",
      "felipe@jsconf.cl",
    ]);
  });

  it("falls back to the calendar title when a calendar has no account", () => {
    const calendars: CalendarRef[] = [{ calendar_id: "birthdays", calendar_title: "Birthdays" }];
    expect(accountsForEvent(calendars, lookup)).toEqual(["Birthdays"]);
  });

  it("returns empty for no calendars", () => {
    expect(accountsForEvent([], lookup)).toEqual([]);
    expect(accountsForEvent(undefined, lookup)).toEqual([]);
  });
});

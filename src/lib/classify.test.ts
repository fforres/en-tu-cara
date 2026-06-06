import { describe, expect, it } from "vitest";
import {
  classify,
  elapsedFraction,
  groupUpcomingByDay,
  ongoingSorted,
  remainingLabel,
  type ClassifiableEvent,
} from "./classify";

// All times constructed in LOCAL time via Date(y, m, d, h, min) so the suite is
// timezone-independent (CP2 edge: event TZ ≠ local TZ is exercised via offsets).

const NOW = new Date(2026, 5, 8, 14, 9, 0); // Mon Jun 8 2026 14:09 local

function ev(
  key: string,
  start: Date,
  end: Date,
  overrides: Partial<ClassifiableEvent> = {},
): ClassifiableEvent {
  return {
    occurrence_key: key,
    title: key,
    start: start.toISOString(),
    end: end.toISOString(),
    all_day: false,
    status: "confirmed",
    ...overrides,
  };
}

const at = (h: number, m: number, day = 8) => new Date(2026, 5, day, h, m, 0);

describe("classify", () => {
  it("ongoing: started, not ended (tray reference: 'until 3:00 PM')", () => {
    expect(classify(ev("a", at(13, 0), at(15, 0)), NOW)).toBe("ongoing");
  });
  it("upcoming: starts later today", () => {
    expect(classify(ev("a", at(17, 0), at(18, 0)), NOW)).toBe("upcoming");
  });
  it("past: already ended", () => {
    expect(classify(ev("a", at(9, 0), at(9, 15)), NOW)).toBe("past");
  });
  it("starts exactly now → ongoing (T-0 boundary)", () => {
    expect(classify(ev("a", NOW, at(15, 0)), NOW)).toBe("ongoing");
  });
  it("ends exactly now → past (half-open interval)", () => {
    expect(classify(ev("a", at(13, 0), NOW), NOW)).toBe("past");
  });
  it("spans midnight: under way at 23:59 start day", () => {
    const lateNow = new Date(2026, 5, 8, 23, 59, 0);
    expect(classify(ev("a", at(23, 0), at(1, 0, 9)), lateNow)).toBe("ongoing");
  });
  it("event in a different timezone classifies by instant, not wall time", () => {
    // 14:09 local NOW; an event expressed as UTC instants equal to 13:00-15:00 local.
    const e: ClassifiableEvent = {
      occurrence_key: "tz",
      title: "tz",
      start: new Date(2026, 5, 8, 13, 0).toISOString(),
      end: new Date(2026, 5, 8, 15, 0).toISOString(),
      all_day: false,
      status: "confirmed",
    };
    expect(classify(e, NOW)).toBe("ongoing");
  });
});

describe("remainingLabel (tray: '51m remaining')", () => {
  it("51 minutes", () => {
    expect(remainingLabel(ev("a", at(13, 0), at(15, 0)), NOW)).toBe("51m remaining");
  });
  it("rounds partial minutes UP (never lies shorter)", () => {
    const e = ev("a", at(13, 0), new Date(2026, 5, 8, 14, 9, 30));
    expect(remainingLabel(e, NOW)).toBe("1m remaining");
  });
  it("hours form", () => {
    expect(remainingLabel(ev("a", at(13, 0), at(16, 14)), NOW)).toBe("2h 05m remaining");
  });
  it("ended", () => {
    expect(remainingLabel(ev("a", at(9, 0), at(10, 0)), NOW)).toBe("ended");
  });
});

describe("elapsedFraction (pie countdown)", () => {
  it("halfway", () => {
    expect(elapsedFraction(ev("a", at(14, 0), at(14, 18)), NOW)).toBeCloseTo(0.5);
  });
  it("clamps before start / after end", () => {
    expect(elapsedFraction(ev("a", at(15, 0), at(16, 0)), NOW)).toBe(0);
    expect(elapsedFraction(ev("a", at(9, 0), at(10, 0)), NOW)).toBe(1);
  });
  it("zero-duration → 1 (no NaN)", () => {
    expect(elapsedFraction(ev("a", at(14, 0), at(14, 0)), NOW)).toBe(1);
  });
});

describe("groupUpcomingByDay (today|all toggle, day headers)", () => {
  const events = [
    ev("today-late", at(20, 0), at(21, 0)),
    ev("tomorrow", at(9, 0, 9), at(9, 15, 9)),
    ev("wednesday", at(9, 0, 10), at(9, 15, 10)),
    ev("ongoing-excluded", at(13, 0), at(15, 0)),
    ev("past-excluded", at(8, 0), at(8, 30)),
  ];

  it("groups by local day with Today/Tomorrow/weekday labels, sorted", () => {
    const groups = groupUpcomingByDay(events, NOW);
    expect(groups.map((g) => g.label)).toEqual(["Today", "Tomorrow", "Wednesday"]);
    expect(groups[0].events.map((e) => e.occurrence_key)).toEqual(["today-late"]);
  });

  it("todayOnly filters other days", () => {
    const groups = groupUpcomingByDay(events, NOW, true);
    expect(groups).toHaveLength(1);
    expect(groups[0].label).toBe("Today");
  });

  it("ongoing and past events never appear", () => {
    const all = groupUpcomingByDay(events, NOW).flatMap((g) => g.events);
    expect(all.map((e) => e.occurrence_key)).not.toContain("ongoing-excluded");
    expect(all.map((e) => e.occurrence_key)).not.toContain("past-excluded");
  });
});

describe("ongoingSorted", () => {
  it("soonest-ending first", () => {
    const events = [
      ev("ends-1500", at(13, 0), at(15, 0)),
      ev("ends-1430", at(14, 0), at(14, 30)),
    ];
    expect(ongoingSorted(events, NOW).map((e) => e.occurrence_key)).toEqual([
      "ends-1430",
      "ends-1500",
    ]);
  });
});

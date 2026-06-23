// Mock data for the DEV preview window (ENTUCARA_PREVIEW=popover → the popover
// UI rendered in a normal resizable window with `?window=popover&preview=1`).
//
// This lets us exercise the list WITHOUT real calendar access (the dev build's
// TCC grant needs a human click): cross-account dedup display + scrolling many
// events + responsiveness at arbitrary window sizes. NEVER imported by the real
// popover path — only when the preview flag is set.

import type { UiEvent } from "./TrayPopover";

interface MockCalendar {
  id: string;
  title: string;
  account: string | null;
  color: [number, number, number, number] | null;
}

const MOCK_CALENDARS: MockCalendar[] = [
  {
    id: "work",
    title: "Felipe Torres Gmail",
    account: "felipe@skyward.ai",
    color: [0.2, 0.45, 0.95, 1],
  },
  { id: "personal", title: "Felipe Torres Gmail", account: "Google", color: [0.25, 0.7, 0.35, 1] },
  { id: "team", title: "Team Events", account: "felipe@skyward.ai", color: [0.95, 0.55, 0.15, 1] },
  { id: "life", title: "Personal", account: "Google", color: [0.6, 0.35, 0.85, 1] },
];

const ZOOM = "https://us04web.zoom.us/j/123456789";
const MEET = "https://meet.google.com/abc-defg-hij";

function iso(base: number, minutesFromNow: number): string {
  return new Date(base + minutesFromNow * 60_000).toISOString();
}

// Builds a realistic-ish snapshot relative to `now`: two ongoing events, the
// cross-account duplicate showcase, and enough upcoming events (across several
// days) to force scrolling.
export function mockPopoverData(now: number): {
  events: UiEvent[];
  calendars: MockCalendar[];
} {
  const base = (
    over: Partial<UiEvent> &
      Pick<UiEvent, "occurrence_key" | "title" | "start" | "end" | "calendar_id">,
  ): UiEvent => ({
    id: over.occurrence_key,
    all_day: false,
    status: "confirmed",
    my_rsvp: "accepted",
    is_recurring_occurrence: false,
    calendar_title: null,
    url: null,
    location: null,
    notes: null,
    calendars: [{ calendar_id: over.calendar_id, calendar_title: over.calendar_title ?? null }],
    ...over,
  });

  const events: UiEvent[] = [
    // --- Ongoing ---
    base({
      occurrence_key: "(focus @ now)",
      title: "Focus block — deep work",
      start: iso(now, -25),
      end: iso(now, 20),
      calendar_id: "team",
      calendar_title: "Team Events",
    }),
    base({
      occurrence_key: "(standup @ now)",
      title: "Eng standup",
      start: iso(now, -5),
      end: iso(now, 25),
      calendar_id: "work",
      calendar_title: "Felipe Torres Gmail",
      notes: `Join: ${ZOOM}`,
    }),

    // --- THE dedup showcase: same meeting on TWO accounts ---
    base({
      occurrence_key: "(adhd @ t1)",
      title: "ADHD medication",
      start: iso(now, 45),
      end: iso(now, 50),
      calendar_id: "work",
      calendar_title: "Felipe Torres Gmail",
      is_recurring_occurrence: true,
      calendars: [
        { calendar_id: "work", calendar_title: "Felipe Torres Gmail" },
        { calendar_id: "personal", calendar_title: "Felipe Torres Gmail" },
      ],
    }),

    // --- Plenty of upcoming events to force scrolling ---
    base({
      occurrence_key: "(1on1 @ t)",
      title: "1:1 with Ana",
      start: iso(now, 90),
      end: iso(now, 120),
      calendar_id: "work",
      calendar_title: "Felipe Torres Gmail",
      notes: `Join: ${MEET}`,
    }),
    base({
      occurrence_key: "(design @ t)",
      title: "Design review — overlay redesign",
      start: iso(now, 150),
      end: iso(now, 210),
      calendar_id: "team",
      calendar_title: "Team Events",
      notes: `Join: ${ZOOM}`,
    }),
    base({
      occurrence_key: "(lunch @ t)",
      title: "Lunch with the team",
      start: iso(now, 240),
      end: iso(now, 300),
      calendar_id: "life",
      calendar_title: "Personal",
    }),
    base({
      occurrence_key: "(gym @ t)",
      title: "Gym",
      start: iso(now, 24 * 60),
      end: iso(now, 24 * 60 + 60),
      calendar_id: "life",
      calendar_title: "Personal",
    }),
    base({
      occurrence_key: "(allhands @ t)",
      title: "Company all-hands",
      start: iso(now, 24 * 60 + 120),
      end: iso(now, 24 * 60 + 180),
      calendar_id: "work",
      calendar_title: "Felipe Torres Gmail",
      notes: `Join: ${MEET}`,
    }),
    // Another cross-account duplicate, a longer title, to test wrapping/badge.
    base({
      occurrence_key: "(planning @ t)",
      title: "Quarterly planning & roadmap sync (long title to test ellipsis)",
      start: iso(now, 24 * 60 + 240),
      end: iso(now, 24 * 60 + 360),
      calendar_id: "work",
      calendar_title: "Felipe Torres Gmail",
      calendars: [
        { calendar_id: "work", calendar_title: "Felipe Torres Gmail" },
        { calendar_id: "team", calendar_title: "Team Events" },
      ],
    }),
    base({
      occurrence_key: "(dentist @ t)",
      title: "Dentist appointment",
      start: iso(now, 2 * 24 * 60),
      end: iso(now, 2 * 24 * 60 + 60),
      calendar_id: "life",
      calendar_title: "Personal",
    }),
    base({
      occurrence_key: "(retro @ t)",
      title: "Sprint retro",
      start: iso(now, 2 * 24 * 60 + 120),
      end: iso(now, 2 * 24 * 60 + 180),
      calendar_id: "team",
      calendar_title: "Team Events",
      notes: `Join: ${ZOOM}`,
    }),
    base({
      occurrence_key: "(coffee @ t)",
      title: "Coffee with Sam",
      start: iso(now, 3 * 24 * 60),
      end: iso(now, 3 * 24 * 60 + 30),
      calendar_id: "life",
      calendar_title: "Personal",
    }),
    base({
      occurrence_key: "(review @ t)",
      title: "Perf review prep",
      start: iso(now, 3 * 24 * 60 + 90),
      end: iso(now, 3 * 24 * 60 + 150),
      calendar_id: "work",
      calendar_title: "Felipe Torres Gmail",
    }),
    base({
      occurrence_key: "(demo @ t)",
      title: "Customer demo",
      start: iso(now, 4 * 24 * 60),
      end: iso(now, 4 * 24 * 60 + 60),
      calendar_id: "work",
      calendar_title: "Felipe Torres Gmail",
      notes: `Join: ${MEET}`,
    }),
  ];

  return { events, calendars: MOCK_CALENDARS };
}

// --- Overlay (takeover) preview ---------------------------------------------

interface MockAlarm {
  occurrence_key: string;
  kind: string;
  title: string;
  start: string | null;
  end: string | null;
}

// Calendars whose TITLES are emails (subscribed colleagues) deliberately share an
// account, so the overlay's account-level origins collapse them — while a genuine
// cross-account duplicate expands to several accounts.
const OVERLAY_CALENDARS: MockCalendar[] = [
  { id: "felipe-primary", title: "felipe@skyward.ai", account: "felipe@skyward.ai", color: null },
  { id: "israel", title: "israel@skyward.ai", account: "felipe@skyward.ai", color: null },
  { id: "google-gmail", title: "FELIPE TORRES — GMAIL", account: "Google", color: null },
  {
    id: "skyward-gmail",
    title: "FELIPE TORRES — GMAIL",
    account: "felipe@skyward.ai",
    color: null,
  },
  { id: "jsconf-gmail", title: "FELIPE TORRES — GMAIL", account: "felipe@jsconf.cl", color: null },
];

// Two stacked cards: a genuine 3-account duplicate (origins expand to three) and a
// colleague-calendar duplicate that collapses to a single account.
export function mockOverlayData(now: number): {
  alarms: MockAlarm[];
  events: UiEvent[];
  calendars: MockCalendar[];
} {
  const event = (
    over: Partial<UiEvent> &
      Pick<UiEvent, "occurrence_key" | "title" | "start" | "end" | "calendars">,
  ): UiEvent => ({
    id: over.occurrence_key,
    all_day: false,
    status: "confirmed",
    my_rsvp: "accepted",
    is_recurring_occurrence: false,
    calendar_id: over.calendars[0]?.calendar_id ?? null,
    calendar_title: over.calendars[0]?.calendar_title ?? null,
    url: null,
    location: null,
    notes: null,
    ...over,
  });

  const alarms: MockAlarm[] = [
    {
      occurrence_key: "(eng @ t1)",
      kind: "t_minus5",
      title: "🔥 ENGINEERING 🔥 Sync",
      start: iso(now, 5),
      end: iso(now, 50),
    },
    {
      occurrence_key: "(basura @ t1)",
      kind: "t_zero",
      title: "Sacar la basura",
      start: iso(now, 1),
      end: iso(now, 16),
    },
  ];

  const events: UiEvent[] = [
    event({
      occurrence_key: "(eng @ t1)",
      title: "🔥 ENGINEERING 🔥 Sync",
      start: iso(now, 5),
      end: iso(now, 50),
      notes: `Join: ${ZOOM}`,
      calendars: [
        { calendar_id: "felipe-primary", calendar_title: "felipe@skyward.ai" },
        { calendar_id: "israel", calendar_title: "israel@skyward.ai" },
      ],
    }),
    event({
      occurrence_key: "(basura @ t1)",
      title: "Sacar la basura",
      start: iso(now, 1),
      end: iso(now, 16),
      is_recurring_occurrence: true,
      calendars: [
        { calendar_id: "google-gmail", calendar_title: "FELIPE TORRES — GMAIL" },
        { calendar_id: "skyward-gmail", calendar_title: "FELIPE TORRES — GMAIL" },
        { calendar_id: "jsconf-gmail", calendar_title: "FELIPE TORRES — GMAIL" },
      ],
    }),
  ];

  return { alarms, events, calendars: OVERLAY_CALENDARS };
}

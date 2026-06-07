// Event classification + display math for the tray popover.
// Pure functions over EventDto-shaped data; `now` is always injected (mock-clock
// discipline — nothing in src/lib may call Date.now() directly).

export interface ClassifiableEvent {
  occurrence_key: string;
  title: string;
  /** RFC3339 — produced by the Rust side from EventKit dates. */
  start: string;
  end: string;
  all_day: boolean;
  status: string; // confirmed | tentative | canceled | none
}

export type Bucket = "ongoing" | "upcoming" | "past";

export function classify(event: ClassifiableEvent, now: Date): Bucket {
  const start = new Date(event.start);
  const end = new Date(event.end);
  // Zero-duration events count as ongoing for the instant of their start.
  if (now >= start && (now < end || (+start === +end && +now === +start))) {
    return "ongoing";
  }
  return now < start ? "upcoming" : "past";
}

/** "51m remaining" / "1h 05m remaining" — the ongoing-row caption. */
export function remainingLabel(event: ClassifiableEvent, now: Date): string {
  const ms = new Date(event.end).getTime() - now.getTime();
  if (ms <= 0) {
    return "ended";
  }
  const totalMin = Math.ceil(ms / 60_000);
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  return h > 0 ? `${h}h ${String(m).padStart(2, "0")}m remaining` : `${m}m remaining`;
}

/** Fraction of the event elapsed, clamped 0..1 — drives the pie countdown. */
export function elapsedFraction(event: ClassifiableEvent, now: Date): number {
  const start = new Date(event.start).getTime();
  const end = new Date(event.end).getTime();
  if (end <= start) {
    return 1;
  }
  return Math.min(1, Math.max(0, (now.getTime() - start) / (end - start)));
}

export interface DayGroup<E extends ClassifiableEvent> {
  /** Weekday header per the tray reference ("Monday"), "Today"/"Tomorrow" first. */
  label: string;
  /** Local date key YYYY-MM-DD, for stable React keys. */
  dateKey: string;
  events: E[];
}

function localDateKey(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/**
 * Group upcoming events by LOCAL calendar day, sorted by start. Events spanning
 * midnight appear under their start day (matching Calendar.app and the tray
 * reference). `todayOnly` implements the today|all toggle.
 */
export function groupUpcomingByDay<E extends ClassifiableEvent>(
  events: E[],
  now: Date,
  todayOnly = false,
): DayGroup<E>[] {
  const upcoming = events
    .filter((e) => classify(e, now) === "upcoming")
    .sort((a, b) => +new Date(a.start) - +new Date(b.start));

  const todayKey = localDateKey(now);
  const tomorrow = new Date(now);
  tomorrow.setDate(tomorrow.getDate() + 1);
  const tomorrowKey = localDateKey(tomorrow);

  const groups = new Map<string, DayGroup<E>>();
  for (const e of upcoming) {
    const start = new Date(e.start);
    const key = localDateKey(start);
    if (todayOnly && key !== todayKey) {
      continue;
    }
    if (!groups.has(key)) {
      const label =
        key === todayKey
          ? "Today"
          : key === tomorrowKey
            ? "Tomorrow"
            : start.toLocaleDateString(undefined, { weekday: "long" });
      groups.set(key, { label, dateKey: key, events: [] });
    }
    groups.get(key)!.events.push(e);
  }
  return [...groups.values()];
}

/** Ongoing events sorted by soonest-ending first (tray top section). */
export function ongoingSorted<E extends ClassifiableEvent>(events: E[], now: Date): E[] {
  return events
    .filter((e) => classify(e, now) === "ongoing")
    .sort((a, b) => +new Date(a.end) - +new Date(b.end));
}

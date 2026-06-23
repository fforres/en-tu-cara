// Resolving which SYNCED ACCOUNTS a (deduped) event is present on — the explicit
// "where did this come from" answer on the takeover.
//
// A meeting that's duplicated surfaces on several calendars; dedup collapses it to
// one row carrying every contributing calendar (`EventDto.calendars`). But the
// user cares about ACCOUNTS, not calendars: a meeting on their own calendar AND a
// subscribed colleague's calendar (whose title is the colleague's email) is still
// just "felipe@skyward.ai" — one account. So we resolve each calendar to its
// account (CalendarDto.account) and dedup at the account level. A genuine
// cross-account duplicate (the same calendar shared into Google + skyward + jsconf)
// yields the distinct account list the user actually wants to see.

export interface CalendarRef {
  calendar_id: string | null;
  calendar_title: string | null;
}

export interface AccountInfo {
  account: string | null;
}

/**
 * Distinct accounts an event is present on, in first-seen order.
 *
 * @param calendars  the event's contributing calendars (EventDto.calendars).
 * @param lookup     calendar id → account (from list_calendars).
 *
 * Falls back to the calendar title only when a calendar has no account at all
 * (e.g. a local "Birthdays" calendar) so we always show *something* meaningful
 * rather than dropping the row silently.
 */
export function accountsForEvent(
  calendars: CalendarRef[] | undefined,
  lookup: Map<string, AccountInfo>,
): string[] {
  if (!calendars || calendars.length === 0) {
    return [];
  }
  const out: string[] = [];
  for (const ref of calendars) {
    const account = ref.calendar_id ? (lookup.get(ref.calendar_id)?.account ?? null) : null;
    const label = account ?? ref.calendar_title;
    if (label && !out.includes(label)) {
      out.push(label);
    }
  }
  return out;
}

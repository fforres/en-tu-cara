// Meeting-link extraction (PLAN §2 Phase 2).
//
// EventKit surfaces the join link wherever the provider stashed it — the URL
// field, the location string, or buried in the notes (often HTML). We scan all
// three in priority order: url > location > notes (CP1a real-data observation:
// Google puts Meet links in location, Zoom invites bury them in notes).

export interface MeetingLink {
  /** Canonical join URL. */
  url: string;
  provider: Provider;
  /** Which event field it came from. */
  source: "url" | "location" | "notes";
}

export type Provider =
  | "zoom"
  | "meet"
  | "teams"
  | "webex"
  | "jitsi"
  | "whereby"
  | "around"
  | "discord"
  | "generic";

// Order matters: first match wins within a field. Specific providers before
// the generic catch-all. Patterns anchored to provider domains, tolerant of
// subdomains (us04web.zoom.us, company.webex.com) and query strings (?pwd=…).
const PROVIDER_PATTERNS: Array<{ provider: Provider; pattern: RegExp }> = [
  // zoom.us/j/<id>, zoom.us/my/<room>, zoom.us/w/<id>, zoomgov.com
  { provider: "zoom", pattern: /https?:\/\/[\w.-]*zoom(?:gov)?\.(?:us|com)\/(?:[a-z]+\/)?(?:j|my|w|s)\/[^\s<>"')\]]+/gi },
  { provider: "meet", pattern: /https?:\/\/meet\.google\.com\/[a-z]{3}-?[a-z]{4}-?[a-z]{3}(?:\?[^\s<>"')\]]*)?/gi },
  // teams.microsoft.com/l/meetup-join/… and teams.live.com
  { provider: "teams", pattern: /https?:\/\/teams\.(?:microsoft|live)\.com\/(?:l\/meetup-join|meet)\/[^\s<>"')\]]+/gi },
  { provider: "webex", pattern: /https?:\/\/[\w.-]+\.webex\.com\/(?:meet|join|[\w-]+\/j\.php)[^\s<>"')\]]*/gi },
  { provider: "jitsi", pattern: /https?:\/\/meet\.jit\.si\/[^\s<>"')\]]+/gi },
  { provider: "whereby", pattern: /https?:\/\/whereby\.com\/[^\s<>"')\]]+/gi },
  { provider: "around", pattern: /https?:\/\/(?:meet\.)?around\.co\/[^\s<>"')\]]+/gi },
  { provider: "discord", pattern: /https?:\/\/discord(?:\.gg|(?:app)?\.com\/channels)\/[^\s<>"')\]]+/gi },
];

// Generic fallback for self-hosted/unknown services (e.g. meet.bman.dev from the
// tray reference): any URL whose host or path smells like a meeting.
const GENERIC_PATTERN =
  /https?:\/\/[\w.-]*(?:meet|call|video|huddle)[\w.-]*\/[^\s<>"')\]]+/gi;

/** Strip trailing punctuation that regexes drag in from prose/HTML contexts. */
function cleanUrl(raw: string): string {
  return raw.replace(/[.,;:!?>)\]}'"]+$/, "");
}

function scanField(
  text: string,
  source: MeetingLink["source"],
): MeetingLink | null {
  for (const { provider, pattern } of PROVIDER_PATTERNS) {
    pattern.lastIndex = 0;
    const match = pattern.exec(text);
    if (match) return { url: cleanUrl(match[0]), provider, source };
  }
  GENERIC_PATTERN.lastIndex = 0;
  const generic = GENERIC_PATTERN.exec(text);
  if (generic) return { url: cleanUrl(generic[0]), provider: "generic", source };
  return null;
}

/**
 * Extract the join link from an event's url/location/notes fields.
 * Priority: explicit URL field > location > notes — the field the user (or
 * provider) most deliberately set wins when several fields carry links.
 */
export function extractMeetingLink(event: {
  url?: string | null;
  location?: string | null;
  notes?: string | null;
}): MeetingLink | null {
  const fields: Array<[string | null | undefined, MeetingLink["source"]]> = [
    [event.url, "url"],
    [event.location, "location"],
    [event.notes, "notes"],
  ];
  for (const [text, source] of fields) {
    if (!text) continue;
    const link = scanField(text, source);
    if (link) return link;
  }
  return null;
}

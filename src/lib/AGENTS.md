# lib/ — internal decisions

Pure functions only: no IPC, no globals, no `Date.now()` — `now: Date` is a
parameter everywhere (mock-clock discipline; tests are timezone-independent by
constructing local `Date(y,m,d,h,min)` values).

- `meeting-links.ts` is the CANONICAL link extractor (UI/Join). The Rust side
  keeps a deliberately dumber copy (`calendar.rs::has_meeting_link`) only for
  the only_video_events alarm policy — if you add a provider, update both.
- Time intervals are half-open: start inclusive, end exclusive.
- `remainingLabel` rounds UP on purpose — never display less time than remains.

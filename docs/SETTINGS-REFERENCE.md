# Settings reference — detailed descriptions

> ⚠️ The original 7 reference PNGs (screenshots of In Your Face's settings windows) were
> accidentally deleted during project scaffolding on 2026-06-05. These descriptions were
> written from detailed inspection of the images before the loss. Re-add screenshots here
> when convenient (the originals were screenshots of In Your Face's settings — easy to retake).
>
> Our app will NOT copy this tabbed layout — we build a **VS Code-style settings page**:
> left sidebar with fuzzy-search box at top + table-of-contents of section titles; typing
> filters and highlights matching settings. The sections and their contents below are what
> we want to support (per Felipe: General, Alerts, Calendars, Event Filters, Appearance,
> Menu Bar, Advanced — NOT Reminders/Shortcuts).

## general.png — "General Settings"

- ☑ Start app at launch
- Open video conferences in: [Default Browser ▾]
- Run Shortcut when joining call: [none ▾] (Apple Shortcuts integration — SKIP for now)

## alerts.png — "Alerts"

- Alert me [1]min [0]sec before the event (numeric steppers)
- ☐ include travel time
- Show alert on: [active screen ▾] (options presumably: active screen / all screens / specific)
- Sound: [Morse ▾] + volume slider + ☑ play repeatedly
- Default Snooze Durations: two sliders → "1 minute", "4 minutes"

## calendars.png — "Calendars"

- Left sub-sidebar: Apps → Calendar / Reminders (we only do Calendar)
- Right: "Active Calendars" — checkbox list GROUPED BY ACCOUNT:
  - felipe@jsconf.cl: FELIPE TORRES — GMAIL, ☑ felipe@jsconf.cl, Holidays in Chile,
    Holidays in United States, Transferred from f@jschile.org, Transferred from felipe@communityos.io
  - felipe@skyward.ai: cristian@skyward.ai, FELIPE TORRES — GMAIL, ☑ felipe@skyward.ai, israel@skyward.ai
  - Each calendar has its color as checkbox tint
- Footer note: "Keep the Calendar app open in the background. This will ensure your events are always in sync."

## event-filters.png — "Event Filters"

- Show events of: [the next 7 days ▾]
- ☐ Work hours — "Display alerts only on the specified days and times." From [9:00 AM] To [5:00 PM], Week days S M T W T F S toggle pills + "all"
- ☐ Only show accepted events — "This will ignore pending and tentatively accepted events."
- ☐ Only show events that have an alert
- ☐ Only show events that have a video conference
- ☐ Show all-day events — at: [9:00 AM]
- Exclusion rules: [Edit…] (title/pattern-based hiding)
- [Manage hidden events…]

## menu-bar.png — "Menu Bar"

- ☑ Show icon — Icon: [👍 ▾] ☑ filled
- ☑ Show next event [24 hours ▾] (show next-event title in the menu bar itself)
- ☐ Show current event
- Title length: [short ▾] + slider (15)
- Countdown: [regular ▾]
- ☑ Indicate if alerts are paused [Customize…]

## appeareance.png — "Appearance" (theme editor)

- Left: theme list (Classic, Classic Light, Dolly, Indian Summer, Monochrome, Retro, TNG, Under the Sea — each "In Your Face" author), + add button
- Middle: theme properties — Name, Author, Alert text color (#hex + swatch), Alert Background
  (Blur mode [none ▾], Color #hex, Opacity %), Action Buttons (Foreground/Background/Opacity),
  Primary Action Button (Foreground/Background/Opacity)
- Right: live preview on a monitor mockup — demo alert: "Hello, I'm a demo event, 3:00 PM – 4:00 PM,
  The event will start in 45 minutes", Join ▾ + Dismiss buttons, Snooze row: [1 minute] [4 minutes] [Until Event]
- [Show Demo Alert] button under the preview

## advanced.png — "Advanced Settings"

- Left sub-sidebar: Alerts / Reminders / Menu Bar / Custom Conference Links
- Alerts pane:
  - ☑ Automatically close alerts — "When on, alerts are automatically closed after the specified timeout." Close alerts after [15] minutes
  - Visuals: ☑ Show alarm clock animation
  - Prompt Personality: slider (🏆 Professional — "Neutral and professional")
  - ☑ Enable "fun" features 🤪 — "This may include little jokes, easter eggs, or other delightful surprises."
- Custom Conference Links (sub-section): user-defined URL patterns for join-link detection

## tray-example.png — tray popover (the PRIMARY MVP reference, image present)

- Header: [+ New Reminder] button, then eye icon (visibility?), pause icon, gear icon
- "ONGOING EVENTS" section: event title, "until 3:00 PM", left color bar (calendar color),
  video-camera icon (green = has link), pie-chart countdown + "51m remaining"
- "UPCOMING EVENTS" section with [today | all] toggle pills
- Grouped by day ("Monday"): each row = recurrence icon (↻) + title, "Jun 8, 2026 at 9:00 AM – 9:15 AM",
  left color bar per calendar, camera icon right (grey when no link), some rows show the raw
  video URL under the time (e.g. "https://us04web.zoom.us/j/4121166431?pwd=…")
- Dark theme, dense list, emoji-heavy titles (🔥 ENGINEERING 🔥 Sync, Felipe Torres 🧠 Atención…)

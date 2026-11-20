### Fixed

- Treat the first-run telemetry decision as the master consent gate. Declining
  it, leaving it unanswered, setting `PERRY_NO_TELEMETRY=1`, or running in CI
  now prevents generic usage events, beta-error reports, and compatibility
  reports from being sent.

- Fail closed for existing configs that have `telemetry.enabled = false` but
  `compatibility_reports = "on"`, and re-check consent immediately before each
  Chirp request so an opt-out made while an event is queued still wins.

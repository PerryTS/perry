### Gates: the gap suite could neither pass nor fail on macOS

`run_gap_tests.sh` selects `test-parity/gap_snapshot.${platform}.json` for
non-Linux hosts. There is no `gap_snapshot.macos.json` in the repo, so every
local macOS run did all ~490 tests' worth of work and then died in a
`FileNotFoundError` — an exit 1 that is indistinguishable from "here is your
regression list". The per-test results were only recoverable by reading the
log by hand, which is exactly how this campaign's verification runs were
actually read.

A missing platform baseline now **says so and falls back to the shared
baseline** — the one the required CI gate uses, so a local run is measured
against the same bar. The fallback is announced on stderr rather than applied
silently, because the platforms genuinely differ (macOS carries known
host-local failures) and quietly comparing against another platform's
expectations would be its own kind of lie. An explicitly-passed
`GAP_SNAPSHOT=` that does not exist is still a hard error (exit 2) — the
fallback is for the unset default, not for a typo.

Found by an agent whose gap run failed this way while validating #7565.

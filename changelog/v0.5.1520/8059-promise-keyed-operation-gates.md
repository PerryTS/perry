### Changed

- Promise keyed-table scaling regressions are now guarded by deterministic
  hash, equality, and displaced-slot operation counts instead of wall-clock
  duration ratios that failed under unrelated host load (#7365).

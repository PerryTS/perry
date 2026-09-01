### Fixed

- Cross-runtime benchmark runners now select an explicit per-fixture integer
  millisecond label and report `UNSCOREABLE` when it is absent or malformed,
  instead of treating the first numeric output line as elapsed time. The
  typed-array fixture keeps its intentional ratio-first #5525 regression
  metric while cross-runtime comparisons use `ta_untyped_access`.

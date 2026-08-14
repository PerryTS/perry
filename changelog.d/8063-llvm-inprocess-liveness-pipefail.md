### Fixed

- The `llvm-inprocess` unit gate no longer reports a false failure when
  `grep -q` finds a required liveness test early and closes a large captured
  output stream under `pipefail` (#8053).

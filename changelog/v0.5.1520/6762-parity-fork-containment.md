### Fixed

- Contain recursively re-executing cluster tests inside a per-test process
  budget and reap their orphaned workers, preventing the Node parity suite
  from exhausting the runner's process limit.

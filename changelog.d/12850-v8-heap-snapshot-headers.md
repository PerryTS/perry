### Fixed

- Match Node's five-field heap snapshot header in `v8.getHeapSnapshot()` and `v8.writeHeapSnapshot()`, while preserving Perry diagnostic metadata for Bun and the public no-argument Rust builder. Extend the existing semantic-schema parity fixture to verify both stream and file header keysets. Refs #9202.

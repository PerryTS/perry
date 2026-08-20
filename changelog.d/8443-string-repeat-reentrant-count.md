### Fixed

- `String.prototype.repeat()` no longer reads a stale string payload when the `count` argument's `valueOf`/`Symbol.toPrimitive` triggers a moving garbage collection. The receiver is now rooted across the coercion and its payload borrowed only afterwards, so a relocated receiver repeats correct bytes instead of from-space garbage. `padStart`/`padEnd` were audited for the same window and are unaffected — codegen runs both of their coercions before the receiver handle is re-read.

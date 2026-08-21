### Fixed

- **GC holders census: pin the two per-thread scanner latches that left `main`
  red.** #8545 taught `scripts/gc_runtime_root_holders.py` to parse
  `perry_thread_local!` declarations, and #8552 replaced process-global scanner
  registration latches with per-thread `Cell<bool>` ones. Each change is correct
  in isolation; together the census gained sight of two latches that no verdict
  covered (`messaging.rs`'s `GC_SCANNER_REGISTERED`, `native_this_alias.rs`'s
  `SCANNER_REGISTERED`), and an unclassified holder fails the gate by design.

  Both are idempotence flags for `ensure_*_scanner_registered()` — they store a
  boolean, never an address — so they are pinned on the identity-ratcheted
  frontier. That placement is forced rather than chosen: they are rule T (the
  census cannot see through the type) and the gate rejects a `holders` verdict
  for a rule-T declaration as stale.

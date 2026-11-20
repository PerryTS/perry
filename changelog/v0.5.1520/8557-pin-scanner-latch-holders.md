### Fixed

- Pin the two thread-local scanner latches #8552 introduced (`messaging.rs`'s
  `GC_SCANNER_REGISTERED`, `native_this_alias.rs`'s `SCANNER_REGISTERED`) in the
  GC root-holder inventory. Both are `Cell<bool>` and cannot hold a heap
  pointer, but the identity ratchet requires every new holder to be recorded.

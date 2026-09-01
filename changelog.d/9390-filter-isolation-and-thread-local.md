### Internal

- **Lands #9383's symbol Bloom-filter isolation**, resolving its conflict with
  the `SymbolAddrRangeGuard::reset()` workaround already on `main` in favour of
  the stronger form: `per_test_global!` isolates `SYMBOL_ADDR_FILTER` alongside
  the `SYMBOL_POINTERS` registry it guards, and the test plants the exact false
  positive on a worker thread. The planted admission leaks through a
  process-global filter and not through the isolated one, so the assertion has
  a subject rather than merely not flaking (#9344).

- **Uses `perry_thread_local!` for `CONCAT_MEMO`.** #9373 declared this hot
  512-entry cache with a raw `thread_local!`, which `check_thread_locals.py`
  rejects — the address should land in the thread's hot cache instead of
  costing a `_tlv_get_addr` call (#7469). Its GC root scanner
  (`scan_concat_memo_roots_mut`) is unaffected.

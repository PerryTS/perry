### Fixed

- **The symbol Bloom-filter probe test no longer depends on unrelated tests'
  filter population.** Test builds now isolate `SYMBOL_ADDR_FILTER` with the
  per-test `SYMBOL_POINTERS` registry it guards, while production keeps the
  same process-global filter. A deterministic worker-thread false-positive
  regression keeps the cross-test leak from returning.

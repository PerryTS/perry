### Fixed

- Root locale comparison strings and options before key allocation and
  getter-driven property access, and reload locale arrays and receiver strings
  after indexed getters can trigger moving GC. This is the moving-GC safety slice
  for #12539; it does not claim broader Intl correctness.

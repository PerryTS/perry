### Performance

- Pre-register immutable pointer-bearing class layouts at module initialization
  and bake their final side-mask state into inline allocation headers. Eligible
  allocations no longer call the typed-layout installer per object, reducing
  retired instructions by 23.9% on `cycles`, 8.3% on `interp`, and 8.1% on
  `iso_miss`, while preserving byte-exact output across the 19-program corpus.

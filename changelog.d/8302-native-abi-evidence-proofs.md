### Fixed

- Restored native ABI evidence for compiler-owned Buffer, typed-array, arena,
  POD-layout, and packed numeric-array values. Buffer numeric reads now retain
  stable pointer facts (including `native_u32`), while optimized native paths
  stay distinct from semantics-preserving erased-annotation fallbacks.

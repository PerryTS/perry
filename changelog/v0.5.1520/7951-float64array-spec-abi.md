### Performance

- **Monomorphic typed-array helpers now preserve raw arguments through their
  specialized ABI (#7221).** An immutable numeric local used as a typed-array
  constructor length now proves the same non-view shape as a direct literal,
  and integer locals with complete write-set proofs cross direct calls as raw
  `i32` values. This lets derived indices remain native inside the helper and
  lowers `Float64Array` writes to direct stores instead of the dynamic setter.

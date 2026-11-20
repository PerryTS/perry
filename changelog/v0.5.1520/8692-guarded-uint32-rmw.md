### Performance

- Fuse dynamic-index `Uint32Array` numeric read-modify-write expressions behind
  explicit representation, backing-store, exact-index, and bounds guards. The
  hot arm keeps the load/add/store in native SSA, while precondition failure and
  post-RHS invalidation retain full JavaScript evaluation order and conversion
  semantics through explicit generic fallbacks.

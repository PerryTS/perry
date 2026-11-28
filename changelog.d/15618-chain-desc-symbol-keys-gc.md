- **Inherited symbol reads after a GC.** `Object.create(P)[sym]` read
  `undefined` once a collection had moved `sym`: the per-prototype lookup
  table kept the symbol's old address. The collector now rewrites those keys.

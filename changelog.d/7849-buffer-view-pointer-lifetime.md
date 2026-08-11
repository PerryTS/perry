## Fixed

- Native-region verification now tracks cached buffer-view pointer lifetime
  independently of bounds and alias facts. Accessing a typed array's `.buffer`
  directly or through a computed or named accessor invalidates every copied
  view alias, runtime fallbacks retain that evidence, and checked or unchecked
  native access through the invalidated pointer is rejected. Statically proven
  canonical numeric string keys remain on the non-invalidating element path.
  Scalar accesses through cached views must carry the same pointer-lifetime
  evidence (#7220).

  Targeted verifier and native-proof regressions cover direct, computed,
  copied-alias, scalar, and bulk-memory access after pointer invalidation.

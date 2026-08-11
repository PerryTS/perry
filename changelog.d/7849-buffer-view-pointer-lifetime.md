## Fixed

- Native-region verification now tracks cached buffer-view pointer lifetime
  independently of bounds and alias facts. Accessing a typed array's `.buffer`
  directly or through a computed key invalidates every copied view alias,
  runtime fallbacks retain that evidence, and checked or unchecked native
  access through the invalidated pointer is rejected. Scalar accesses through
  cached views must carry the same pointer-lifetime evidence.

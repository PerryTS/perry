## Fixed

- Native-region verification now tracks cached buffer-view pointer lifetime
  independently of bounds and alias facts. Accessing a typed array's `.buffer`
  records that its inline-storage pointer was invalidated, runtime fallbacks
  retain that evidence, and checked or unchecked native access through the
  invalidated pointer is rejected.

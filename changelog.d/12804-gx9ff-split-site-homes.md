Constructors and other functions with many property stores compile smaller
again: a GC root that stays live across a store's slow path keeps one stack
slot instead of being copied around every slow call. tsc's code shrinks by
about 575 KB.

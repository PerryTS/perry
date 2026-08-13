**Fixed: the zod dependency corpus no longer reaches the young-pin latch
through a stale comparison operand (#7990).**

The abort's `GC_TYPE_MAP | GC_FLAG_INTERNED` header was internally impossible:
the interned flag is only created on strings. A comparison operand had kept an
old SSA address across an allocating right-hand operand, then handed recycled
bytes to the copying collector. The comparison-rooting fix in #8011 removed
that window; a patch-only A/B completed 26 of 26 rate-1 corpus runs clean, with
about 6,400 copying minors and 855,000 moved objects per run.

An LLVM-dataflow regression now covers the reported typed-Map population and
proves that its value is re-read from a GC root below the allocating operand.
The collector still aborts before an unsafe relocation, but its internal
documentation no longer assumes an incomplete pin latch is the only possible
cause.

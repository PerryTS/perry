Fix three GC-safety and crash findings from the #11680 audit.

- Imported constructors (`new ImportedClass(...)`) no longer pass stale
  argument pointers across a moving collection. Positional arguments and the
  values packed into a `...rest` or synthetic `arguments` array were read
  once, before the array allocation, the pushes and the class-value lookup.
  Each one is now re-read from its root at its use: every push re-reads its
  element, and the dispatch re-reads every positional argument. The
  whole-function `root_reload` pass was already repairing the emitted IR for
  these shapes; the lowering no longer depends on it, and its test now also
  checks the lowering with that pass switched off (a test-only seam).
- `publish_key_add_edge` roots the receiver as well as the closure. The GC
  call-effects classifier cannot prove a shape mint non-collecting, so the
  ConstFn prewrite, the SPECIAL stamp and the key-add convergence re-read the
  receiver after each publication instead of writing through the pre-mint
  pointer. The static finalizer's comment that called the mint
  non-collecting is corrected.
- The static ConstFn finalizer no longer aborts the process when a receiver
  has the same key names as the record already seeded under the requested
  static id but a different keys array. It compares the complete record,
  keys identity included, before minting: a match is stamped, and anything
  else is a refusal that leaves the receiver untouched.

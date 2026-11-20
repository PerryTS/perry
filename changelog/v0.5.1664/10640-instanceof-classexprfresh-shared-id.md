### Fixed

- `instanceof` against a `ClassExprFresh` parent (a heritage-carrying class
  expression — captures, statics, private elements, or a self-binding)
  resolved the dynamic parent by the SHARED template `class_id` rather than
  per evaluation. An instance built from an EARLIER evaluation of a
  repeatedly-evaluated factory, constructed after a LATER evaluation of the
  same factory had run, constructed with the correct parent (a prior fix,
  #9364/#6438, already gives each evaluation its own pinned heritage for
  `super()`/capture resolution) but could fail `instanceof` against its own
  true parent — the later evaluation's parent shadowed it in the shared,
  last-write-wins `CLASS_REGISTRY` that `instanceof`'s class-chain walk read.
  Fixed by pinning the constructing class object onto each new instance too
  (`class_registry/evaluation_heritage.rs`) and giving `instanceof`'s chain
  walk a value-aware path (`instanceof.rs`'s `class_chain_reaches_dynamic`)
  that prefers a pinned VALUE at each hop over the shared class_id table,
  gated behind a monotone latch so the common (never-evaluated-twice) case
  is unaffected. Runtime-only; no codegen changes. (#10624)

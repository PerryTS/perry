### Documentation

- **Engine plan synced to v0.5.1345 (#7607).** Adds the one-table summary of
  the 2026-08-07/08 wave — twenty-two merges: the `json_pipeline` scaling cliff
  taken 97.6× → ~8× bun across four PRs (#7594 livelock latch, #7596
  live-proportional budgets, #7601 `charCodeAt` inlining, #7600 push-length
  elision), the array-push write barrier gated on the parent header (#7602),
  two subclass memory-safety families closed (#7573 Map/Set SIGBUS, #7603
  Array SIGSEGV, #7605 inherited statics), two class-id collisions fixed with a
  scanning `lint` gate (#7583/#7589), `known_failures.json` converted from a
  suppression list to a self-emptying ratchet (#7599), and the public baseline
  published for the first time in five days (#7593).

  Records the named fix pattern for the *declared-type-as-layout-proof* bug
  class — runtime-funnel brand checks **plus** codegen-tier guards, because the
  inline-store tiers never reach the runtime — and the ordering constraint that
  **#7554 (gc-ratchet CI repair) must precede the next GC-pacing change**:
  #7594 and #7596 both had to substitute hand-run both-arms A/Bs for the broken
  official gate, and a third pacing change without the gate would be the
  "measured nothing" hazard with a paper trail.

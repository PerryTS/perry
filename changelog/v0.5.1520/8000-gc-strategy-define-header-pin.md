### Codegen: pin the GC strategy onto the define line, in both root lowerings (#7982 follow-up)

#7998 made `LlFunction::define_header` the single renderer of the `define … {`
line, after the in-process native path's private copy silently lost
`gc "statepoint-example"` — a natively-constructed module then got no RS4GC
pass and therefore **no precise roots at all**, while verifying, linking and
running correctly on any program that does not collect.

The test that shipped with it does not actually pin that property, and the two
attempts to fix it failed the same way the original bug did. Recorded because
it is this change's own bug class occurring inside its own test:

1. The `to_ir` == `define_header` agreement test **cannot** see a dropped
   strategy: with one shared renderer both sides change identically. Sabotage
   passed.
2. A dedicated strategy test that branched on `native_stack_roots_enabled()`
   never ran its ON arm under `cargo test` — no module has called
   `set_native_roots_for_target`, so the predicate is false in the test
   process. Sabotage passed again.
3. Only pinning **both** lowerings with `NativeRootsPin::{native,shadow}`, and
   asserting `stack_map_slot_count` in each arm first so neither can pass
   vacuously, goes red on the sabotage.

Native-roots must take the stack-map path AND name the strategy; shadow-stack
must not take it AND must not name the strategy. The tests live in
`function.rs`, which compiles WITHOUT the `llvm-inprocess` feature, so they run
in per-PR `cargo-test` rather than only in the feature job.

Sabotage-verified with the fix committed first, then restored and REBUILT: 935
`perry-codegen` lib tests green under `--features llvm-inprocess`.

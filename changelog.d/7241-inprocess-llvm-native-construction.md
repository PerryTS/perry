Opt-in in-process LLVM backend (#7241, engine-plan layer 0): with the
`llvm-inprocess` cargo feature built in and `PERRY_LLVM_INPROCESS=native`,
Perry constructs LLVM modules through the C API against pinned LLVM 22 — no
module-scale IR text, no `.ll` on disk, no `clang` subprocess, no dependence
on the user's toolchain. All 68 `LlBlock` semantic methods emit typed
instructions that build directly (`inst.rs`/`dialect.rs`); codegen-unit
splits build per-unit native modules and partial-link as before; `=diff`
mode byte-compares the two backends' objects (both corpora byte-identical);
`PERRY_SAVE_LL`/`--trace llvm`/`PERRY_LLVM_KEEP_IR` print the constructed
module as the debug view. `=1` selects the transport-only mode (whole-module
text parsed in-process). The flag participates in the build- and
object-cache keys; a build without the feature fails loudly if the flag is
set. The default build is unchanged: no LLVM link dependency, and emitted
IR is byte-identical to the merge-base (verified over a 12-file corpus; the
only divergence class is a pre-existing run-to-run closure-name
registration ordering coin that flips within a single binary).
`RewriteStatepointsForGC` scheduling in-process is pinned by test —
the layer-2 (#7174) unblock this branch exists for.

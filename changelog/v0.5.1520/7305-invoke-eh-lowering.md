### Exception lowering: setjmp/longjmp → LLVM `invoke`/`landingpad` (#7302)

`try`/`catch` (and the async rejection boundary) now lower to real LLVM
unwind edges instead of `setjmp`/`longjmp`. One root cause, three problems
collapsed:

1. **Moving-GC soundness in `try` functions.** A `longjmp` could jump past a
   `gc.statepoint`'s relocation write-back, which is why the statepoint
   experiment (#7174) excluded `has_try` functions and fell back to an
   unsound plain-stack-map lowering. With explicit unwind edges, relocations
   exist on both the normal and unwind paths and no jump can skip one — the
   GC branch can delete its `has_try` exclusion and the fallback entirely.
2. **~570 lines of register-allocator appeasement deleted.**
   `volatile_setjmp.rs` (376) + `setjmp_abi.rs` (193) existed to implement
   C99 7.13.2.1p3 (volatile promotion of try-mutated locals). Gone, along
   with the `#0 returns_twice`/`#1 noinline` attribute groups.
3. **`try` functions now optimize.** No `returns_twice` barrier, no
   `noinline`, no volatile-pinned locals: a function containing `try` can
   inline and its locals live in SSA.

Transport: `js_throw` stores the value in the GC-rooted TLS slot (unchanged),
applies the savepoint restores (unchanged), then raises a payload-free
`PERRYJS\0` `_Unwind_Exception` through a Perry-owned Itanium personality
(`perry_eh_personality`, an LSDA walk ported from Rust std). Landing pads are
catch-all and read the value back via `js_get_exception` — the catch-entry
sequence is bit-identical to the setjmp path. On windows-msvc the same
lowering emits SEH funclets (`catchswitch`/`catchpad`, personality
`__C_specific_handler`, filter on `RaiseException` code `0xE0504A53`).
Rust-side boundary traps (`js_call_catching`, combinators, iterator/timer
trampolines) keep their private `ffi::setjmp` — Rust cannot catch a foreign
exception, and an open Rust handler is always innermost when it is the throw
target, so a raise never crosses one.

Build contract: the runtime archives are built `panic=abort` with
`-C force-unwind-tables=yes` — measured as the only configuration where the
unwinder steps runtime Rust helper frames with exactly `longjmp` semantics
(no cleanups, so the at-throw savepoint restores stay correct; under
`panic=unwind` the RFC-2945 abort guards on `extern "C"` helpers abort the
process instead). A once-per-process `_Unwind_Backtrace` self-check on the
first `try` aborts loudly if a stray `RUSTFLAGS` dropped the flag, instead of
stranding the first cross-helper throw.

Tooling made invoke-aware: `scripts/gc_root_dominance_check.py` (CALL_RE +
invoke CFG edges — collecting calls inside `try` bodies stay visible),
`LlBlock::contains_gc_unsafe_call`, and a render-time phi-predecessor rewrite
for the inline `eh.contN` block splits.

New gap coverage: structural path matrix
(`test_gap_7302_invoke_eh_paths.ts`), throws crossing runtime helper frames
(`test_gap_7302_throw_across_helper_frames.ts`), and the previously-missing
GC probe that allocates inside `try` and throws across a collection point
(`test_gap_7302_gc_throw_across_collection.ts`).

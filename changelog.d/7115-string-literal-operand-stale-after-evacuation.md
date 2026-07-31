### Fixed

- **A string-literal operand was reused from a register across an allocating
  sibling, so an evacuating collection silently truncated the result (#7114).**
  `console.log("acc:" + run(10_000_000))` printed an **empty line and exited 0**
  — no crash, no diagnostic. The corruption was allocation-count dependent
  (correct at 100 000 iterations, prefix replaced by a garbage byte or gone
  entirely at 10⁷), which is the signature of a stale heap address rather than a
  logic error. Hoisting the call into its own statement made it correct, which
  localised it to operand evaluation order rather than to `run`, to
  `js_string_concat_value`, or to the arithmetic.

  In the emitted IR the literal's `__perry_init_strings_*` handle was loaded
  *above* the call and masked to a pointer *below* it:

  ```llvm
  %r1 = load double, ptr @m_ts_.str.1.handle          ; read BEFORE
  %r2 = call double @perry_fn_m_ts__run__spec_i32(i32 10000000)
  %r3 = bitcast double %r1 to i64                     ; STALE
  %r4 = and  i64 %r3, 281474976710655
  %r5 = call i64 @js_string_concat_value(i64 %r4, double %r2)
  ```

  The handle global *is* a registered GC root, so the string was never swept and
  the global was rewritten when the copying minor relocated it. The register
  taken beforehand was not.

  **Root cause: two implementations of one contract, drifted.**
  `crates/perry-codegen/src/expr/temp_root.rs` suppresses a string literal from
  temp rooting — correctly; it is already a root and cannot be swept — and
  compensates by re-deriving it below the collection point. `RootedOperands`
  (`new C(a, b)`, native collection methods) did both halves. `lower_exprs_rooted`
  — behind `lower_operand_pair_rooted`, the array-literal element list and the
  string-concat chain — did only the suppression. Its own comment said the
  staleness "is not the hazard #6951 is about"; it was #7114.

  **The invariant this establishes**, now stated in the module header: *no
  operand register may outlive a collection point — after the last thing that
  can collect, every operand is either re-read from a root the collector rewrote
  or re-derived from immutable storage, never reused.* A root buys three things
  and they are not the same: liveness, a rewritten location, and **the value the
  consuming call actually observes**. The third is the one #7114 dropped.

  The fix routes both helper families through one `operand_protection()`
  decision (`Root` / `Reload` / `Reuse`) so the pair cannot drift again. This is
  the codegen shadow-stack/temp-root mechanism, not `RuntimeHandleScope`: the
  stale value lived in an LLVM SSA register in generated code, so no runtime
  helper's handle scope could have seen it.

  Cost is zero runtime calls — `Reload` re-emits the `load` that was going to be
  emitted anyway, and only when a later operand can collect, so `"user_" + i`
  and every other non-collecting concat keep their previous IR byte for byte.

  Verified on an M1 at `--release` against Node 26.5.1: the new gap probe prints
  `!119999700000` on `ff85fd483` and `acc:119999700000` after, with 60 GC cycles
  and 2 255 476 objects relocated by the copying minor in the same run.

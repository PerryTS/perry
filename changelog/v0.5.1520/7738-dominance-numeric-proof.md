### Fixed

- **`gc-root-dominance-statepoints` was red on `main`, on nine false positives (#7738).** #7732 drove the native lowering's unrooted count to 0 and — correctly, per #7706's precedent — **deleted** `--max-unrooted` rather than setting it to 0. With no budget, any spurious hit is a red build, and nine arrived.

  Every one had the same shape: a `load double` from a module global (`@perry_global_…`), reported as a stale register because the load crosses a statepoint, whose **only use is float arithmetic**:

  ```
  source (global): %r226 = load double, ptr @perry_global_…_ts__1, align 8
  stale use       : %r309 = fadd double %r226, %r308
  ```

  The global is `let churnAcc = 0` in `test_gap_repsel_gc_stress` — a number. Perry represents every JS value as a NaN-boxed double, so "it is a double" says nothing about whether it is a pointer; but float **arithmetic** does. A NaN-box carries its tag in the exponent/mantissa bits and `fadd` destroys it, so codegen emits one only where it has already proven the operand numeric. **The instruction is the proof.** A stale copy of a number is just a number: nothing to rewrite, nothing to dereference.

  `fcmp` gets exactly the **ordered** predicates (`oeq ogt oge olt ole one ord`). An ordered comparison is false unless both operands are non-NaN, and every NaN-boxed reference is a NaN — so an ordered predicate on a boxed pointer is a constant `false` that codegen has no reason to emit. The **unordered** ones are excluded deliberately: `fcmp uno` is precisely how a NaN-box tag check is written, and treating it as numeric proof would blind the checker to the pointer case it exists for.

  The filter is **per-use, not per-source**. A register with one arithmetic use and one dereference is still reported for the dereference — the case that matters, and the one a source-level filter would have silently dropped.

  Verified in both directions rather than assumed. `--self-test` still passes, so the checker can still fail. And the predicate was exercised against ten instruction shapes: `fadd` / ordered `fcmp` / `fmul fast` are filtered; `js_object_get_field_by_name_f64`, `bitcast … to i64`, `store`, `fcmp uno`, `fcmp une`, `js_nanbox_get_pointer` and `inttoptr` are all still reported. Corpus: `131/131 sources compiled, 0 skipped` → `within budget: unrooted 0 <= 0`.

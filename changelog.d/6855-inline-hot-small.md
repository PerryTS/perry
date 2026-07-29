### Changed

- Inline small **hot** (in-loop) functions into their loops — the residual gap
  a tight integer-math kernel had vs V8 after #6850 (native `Math.imul` +
  typed-array-param reads) closed the codegen gap but left the *call itself*
  out-of-line. Perry force-inlines functions `<= 8` statements with
  `alwaysinline` (unconditional); a NaN-boxed bit-mixer like `mix` is ~10
  statements and costs ~800 in LLVM's inline model (GC shadow-frame calls +
  typed-array reads + double<->i32 marshaling), above `-O3`'s base threshold, so
  it stayed a call.
  - **Bias, don't force.** A distinct `inlinehint` attribute path (separate from
    `alwaysinline`) is now stamped on functions that are *small*
    (`9..=SIZE_CAP` statements), *hot* (≥1 call site inside a loop — a
    whole-module HIR pre-pass, `collectors/hot_callees.rs`, collects such callee
    ids), AND called from *few* total sites. A function only ever called from
    cold/straight-line code is never hinted, so cold utilities can't bloat the
    binary.
  - The linker raises LLVM's `-inlinehint-threshold` (default 850) so hinted
    kernels actually inline. This lifts the ceiling **only** for functions Perry
    marked `inlinehint`; every other function keeps the base `-O3` threshold.
  - **Anti-bloat backstop — the call-site cap.** `-inlinehint-threshold` raises
    the ceiling at *every* call site of a hinted callee (LLVM can't tell hot
    from cold per-site through a function attribute), so a hot kernel also
    called from many cold sites would be duplicated at all of them. Capping
    total call sites (default 4) bounds the duplication. A synthetic of 300 fns
    × 40 cold sites hinted *without* the cap grew its optimized IR 5.7×
    (205K → 1.17M lines); *with* the cap those broadly-called fns are excluded,
    for a 0% delta. Measured binary-size deltas (flag OFF vs ON, byte-precise
    `__text`): real programs and >4-call-site helpers 0.000%; large programs
    (>6MB IR → `-Os`) 0.000% (flag inert); realistic hot-kernel densities +0.34%
    (25 kernels) / +0.79% (50) / +1.57% (100). `noinline` cases
    (try/setjmp/volatile) are respected via `to_ir`'s `has_try`-first attribute
    precedence.
  - On the 40M-call `mix` microbench, the caller loop no longer emits a `bl` to
    `mix` (0 vs 2 `BR26` relocations) and `mix` carries `inlinehint`; wall-clock
    fell from ~1024 ms to ~904 ms (min-of-7, contended box).
  - Gated behind `PERRY_INLINE_HOT_SMALL` (default on);
    `PERRY_INLINE_HOT_SMALL_CAP` / `_THRESHOLD` / `_MAX_SITES` tune the size
    window, hint threshold, and call-site cap. All four are folded into the
    object cache key. New `test-files/test_gap_inline_hot_small.ts` asserts
    byte-identical output to Node with the flag on, off, and under
    `PERRY_GC_FORCE_EVACUATE=1`.

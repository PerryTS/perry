### Fixed

- **`compiler-output-regression`: the HIR probe no longer blocks on a nested
  cold `cargo build` (#10782).** `scripts/compiler_output_harness/capture.py`
  built every `perry compile` environment as `{**os.environ,
  PERRY_LLVM_KEEP_IR, PERRY_NO_CACHE}` and never set `PERRY_NO_AUTO_OPTIMIZE`.
  Auto-optimize therefore fired on the `--print-hir --no-link` probe and
  blocked on `cargo build --release -p perry-runtime-static -p
  perry-stdlib-static` — a from-source runtime build — inside the 300-second
  `--compile-timeout`. The compiler sat at 0% CPU with a `cargo` child while
  the harness killed it, so the traceback read like a compiler hang. Measured
  on v0.5.1611: the probe takes **1.4 s** with the variable set and **still had
  not finished at 60 s** without it; the nested build alone completes in
  **343 s** on an M-series mini, i.e. it cannot fit the budget it was charged
  to.

  The variable is set **only on the HIR probe**, not globally. The two `perry
  compile` calls in `capture()` are different kinds of step, and only one is
  inert:

  - the `--print-hir --no-link` probe produces no binary and links no runtime,
    so nothing it feeds (`hir.txt`) can observe which runtime auto-optimize
    would have built — the same reasoning `compile_and_census` already records
    for its own `--no-link` compile;
  - the linking compile produces the binary `run_benchmark` / `run_perf_stat`
    then execute, and the `allocations_traced` / `gc_collections_traced` /
    `write_barriers_traced` budgets are read back out of that binary's
    `PERRY_GC_TRACE` stderr. Auto-optimize is what puts the trace there:
    `optimized_libs/freshness.rs` adds `perry-runtime/diagnostics` to the
    rebuild when the compiler sees `PERRY_GC_TRACE=1`, and that derivation runs
    only on the auto-optimize path. Those budgets are **maxima**, so a runtime
    that emits no trace scores 0 on all three and passes every one of them
    vacuously — suppressing there would have left the gate green while it
    measured nothing.

  `_compile_env` grows a `suppress_auto_optimize` keyword that defaults to OFF,
  so the inertness argument has to be made per call site rather than inherited.

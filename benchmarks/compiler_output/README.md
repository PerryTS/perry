# Compiler Output Regression Harness

This harness records the evidence chain for CPU-sensitive Perry benchmarks:

```text
HIR -> LLVM IR before opt -> retained object -> object disassembly -> benchmark result
```

Run the primary image convolution gate:

```bash
python3 scripts/compiler_output_regression.py capture \
  --workload image_convolution \
  --gate \
  --print-summary
```

Artifacts are written under `target/compiler-output-regression/<workload>/` by
default, including `hir.txt`, `llvm-before-opt.ll`, retained `object-*.o`
files, `object-*.compile-plan.json`, `object-disassembly.s`,
`llvm-after-opt.analysis.ll`, `llvm-vectorization-remarks.stderr`,
`manifest.json`, and `structural-report.json`. The optimized IR file is an
analysis-only artifact; assembly and FMA gates are counted from the retained
object disassembly.

The structural gate checks that hot loop blocks keep native-shaped buffer and
numeric code: direct `getelementptr inbounds`, `llvm.assume`, invariant-load
and alias/noalias metadata are required, while runtime helper calls,
NaN-boxing-style fallback paths, repeated FP/int conversions, dynamic property
lookups, boxed number allocation, and buffer slow-path helpers fail the report.
Runtime/GC isolation is enforced through per-workload budgets in
`runtime_counter_summary`, so setup-only costs can be documented while hot
regions still fail on allocations, GC, write barriers, boxed numbers, and
buffer slow paths.

Vectorization and workload contracts are policy driven by
`benchmarks/compiler_output/workloads.toml`. `structural-report.json` records
the observed missed-vectorization reason kinds and fails if a workload loses a
required vectorized loop or starts reporting an unapproved reason such as
aliasing. Scalar baselines, including `loop_data_dependent`, are explicit in
the spec instead of implicit `vectorized_count: 0` passes. The
`vectorized_buffer_transform` fixture is the positive gate: it requires at
least one LLVM loop-vectorizer success remark.

The harness also captures best-effort explanation counters:

- IR/assembly structural counters, including runtime calls, conversions,
  write-barrier helper calls, boxed-number allocation helpers, buffer slow-path
  calls, FMA/SIMD instruction mentions, and assembly call instructions.
- LLVM loop-vectorization remarks via `-Rpass=loop-vectorize`,
  `-Rpass-missed=loop-vectorize`, and `-Rpass-analysis=loop-vectorize`.
- Runtime GC/write-barrier/allocation trace summaries when `PERRY_GC_TRACE=1`
  produces JSON events.
- A `runtime_counter_summary` block with runtime calls, allocations, GC
  collections, write barriers, boxed-number allocations, and buffer slow-path
  accesses summarized for each benchmark capture.
- Benchmark timing stats: median, mean, min, max, p95, standard deviation, run
  count, and whether the capture is a CI smoke run or timing-quality run.
- The real Perry object compile plan: clang path, effective target, clang
  arguments, native tuning flag, retained object path, and clang stderr path.
- Hot-loop region counters under `regions.hot_loops` plus semantic
  `regions.named` entries for image input generation, blur, FNV hashing, and
  numeric loop bodies. Each named region has its own structural contract in
  the TOML spec.
- Hardware counters from `perf stat` when available on Linux.

## Native ABI Evidence Packet Matrix

`scripts/native_abi_evidence_packet.sh --gate` (with `--runs >= 5`) aggregates the
`native-abi-proof` compiler-output suite into
`native-abi-evidence.json` and `native-abi-evidence.md`. The packet is the
representative material type-lowering gate for PRs and release sweeps.

| Gate row | Evidence | Required proof |
|---|---|---|
| Native ABI correctness | `tests/test_native_abi_contract.sh` and `tests/test_c_layout_pod_records.sh` retained native-rep artifacts | runtime output passes and required ABI/materialization tokens are present |
| Native-region artifact chain | retained HIR, LLVM before opt, LLVM after opt analysis, object disassembly, compile plans, and native-rep JSON | artifacts exist, structural safety checks pass, semantic checksum checks pass |
| Explain-lowering accounting | native-rep rows summarized into boxes, unboxes/coercions, dynamic fallbacks, barrier decisions, typed native records, and runtime counter summaries | typed/control material accounting rows pass |
| Runtime safety | `perry-runtime native_async` tests | required native async/rooting test names pass and appear in logs |
| Release/LTO symbols | `scripts/check_runtime_symbols.sh` over the runtime archive | runtime archive defines all sentinel symbols |

The material accounting rows compare
`native_abi_packet_typed.ts` against `native_abi_packet_control.ts`.
The gate requires 100% elimination of boxed-number allocations, Buffer
slow-path helpers, and typed-array/Uint8Array slow-path helpers in the typed
packet; at least 95% fewer traced allocations and traced write barriers; at
least 75% fewer static write-barrier helper sites; at least 25% fewer static
runtime helper call sites; at least 2.0x median wall-time speedup; and at least
1.5x p95 wall-time speedup. The control packet must keep positive boxed,
helper, barrier, allocation, and runtime-call baselines, and both packets must
produce matching semantic checksums so the optimized path and fallback path are
comparing the same work.

The `hir_fact_rewrite` fixture is the rewrite-insensitivity gate for the HIR
fact layer. It keeps `const j = helper(i); dst[j] = ...` on the same direct
buffer path as an inline index expression: inbounds GEPs, bounds assumptions,
alias/noalias metadata, zero buffer slow-path helpers, and at least one
vectorized loop are required.

CI uses the smoke profile to keep pull requests fast:

```bash
python3 scripts/compiler_output_regression.py capture \
  --workload image_convolution \
  --benchmark-mode smoke \
  --runs 1 \
  --perf-counters off \
  --gate
```

Local and release timing captures should omit `--runs` and select a stronger
profile:

```bash
python3 scripts/compiler_output_regression.py capture \
  --workload image_convolution \
  --benchmark-mode standard \
  --gate
```

Floating-point modes can be captured explicitly:

```bash
python3 scripts/compiler_output_regression.py capture \
  --workload fma_contract \
  --fp-contract=on \
  --clang-arg=-march=haswell \
  --expect-fma=on \
  --gate
```

`--clang-arg` is kept for compatibility and only affects the analysis-only
optimized IR emission. FMA instruction gates read the retained Perry object
compile plan from `manifest.json`; host captures use native CPU tuning unless
an explicit target triple is supplied.

The no-contraction case remains a separate gate even when fast math is enabled:

```bash
python3 scripts/compiler_output_regression.py capture \
  --workload fma_contract \
  --fast-math \
  --fp-contract=off \
  --clang-arg=-march=haswell \
  --expect-fma=off \
  --gate
```

For a compile-only smoke run:

```bash
python3 scripts/compiler_output_regression.py capture \
  --workload loop_data_dependent \
  --skip-run \
  --gate
```

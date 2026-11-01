# Cold runtime size optimization (#13662)

## Scope

Keep the runtime's speed optimization policy and add `#[optimize(size)]` only
to 198 existing `#[cold] #[inline(never)]` functions (121 files). These are
already separated slow paths, diagnostics, initialization and error helpers.
There are no algorithm, object-layout, rooting, dispatch, side-table or cache
changes. The default Cargo release profile and unwind configuration are unchanged.

This is a conservative size reduction, not the issue's unachieved 4.43 MiB
full-application target. A cold annotation is a heuristic, so the broader
workloads below are part of the evidence rather than assuming cold means free.

## Controls

Reference: `dcc57549822d0b8f33048fdfbd2e857ac62371a5` (version 0.5.1663).
Candidate: that source plus this PR's runtime attributes. macOS 26.5,
Apple M1 Max, ARM64; pinned Rust nightly-2026-10-04 (1.101), LLVM 22.1.8
for application code. Independently built reference and candidate static
wrapper archives; one compiler built from the reference source for all arms.
Every link pins `PERRY_RUNTIME_DIR`; archive fingerprints and generated
GC-effects sidecars match. Application optimization is O2, specialized ABI 1,
system allocator, fat LTO. Repository synchronous unwind tables are retained.
Application compilation disables the build cache and automatic optimization.

The public scan uses `keepalive-anchors` and stdlib `async-bridge`. The regular
suite and original chess application add `regex-engine` and `temporal` on both
arms. These are explicit feature sets, not full default-feature runtimes.
The blanket-Oz control changes the runtime/stdlib profile, including
dependencies, to Oz while retaining app O2. The candidate retains O2 except
for the explicit cold helpers. Dependency-only Oz was also tried and rejected:
it produced no meaningful stripped-size saving for this fixture.

See [the reproduction tools](../../benchmarks/runtime-size/README.md). Their
builder was exercised against the candidate and reproduced both measured
archive hashes. The fat-LTO effects-rule adaptations are the issue's separate
metadata compatibility workaround; unknown-call checks and fingerprints
remain enforced.

## Measurements

Nine rotated rounds of the public scan (5M probes per sample). All results
match `warmup=42188`, `checksum=2109378`. Retired instructions and cycles below
are billions; times and counts are medians.

| Mode | Runtime | Stripped bytes | Instructions G | Cycles G | Inner ms | Process s |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| forof | o2 | 5,634,688 | 5.39052 | 1.02484 | 383.82 | 0.40 |
| forof | cold | 5,602,240 | 5.39042 | 1.02508 | 374.29 | 0.39 |
| forof | oz | 3,369,840 | 5.98026 | 1.60832 | 648.45 | 0.67 |
| indexed | o2 | 5,634,688 | 4.25756 | 0.77408 | 288.95 | 0.30 |
| indexed | cold | 5,602,240 | 4.25780 | 0.78375 | 290.12 | 0.30 |
| indexed | oz | 3,369,840 | 4.38600 | 0.85504 | 301.56 | 0.31 |

The candidate saves **32,448 bytes (0.58%)**. Its scan instructions differ by
less than 0.01% from O2. This is much smaller than blanket Oz's size reduction.
The latter still increases retired instructions and cycles in both modes.

### Regular suite

Five rotated rounds of all 30 `benchmarks/suite/*.ts` fixtures; all non-timing
output matches. Percentages compare candidate medians to reference. Very
short inner timers are millisecond-quantized. Raw CSVs retain absolute counts,
whole-process wall time and binary bytes for every sample.

| Fixture | Bytes saved | Instructions Δ | Cycles Δ | Inner ms, reference → candidate |
| --- | ---: | ---: | ---: | ---: |
| 01_startup | 32,432 | -0.38% | -9.84% | 10 → 10 |
| 02_loop_overhead | 32,432 | -0.04% | -0.50% | 148 → 148 |
| 03_array_write | 32,432 | -0.38% | +1.41% | 25 → 26 |
| 04_array_read | 32,432 | -0.67% | -3.92% | 30 → 26 |
| 05_fibonacci | 32,384 | +0.00% | +0.04% | 718 → 788 |
| 06_math_intensive | 32,448 | +0.08% | -0.64% | 57 → 61 |
| 07_object_create | 32,432 | +0.42% | +7.37% | 5 → 5 |
| 08_string_concat | 32,432 | +0.06% | +3.83% | 6 → 6 |
| 09_method_calls | 32,432 | +0.06% | +1.95% | 38 → 39 |
| 10_nested_loops | 32,432 | -0.13% | +2.18% | 19 → 19 |
| 11_prime_sieve | 32,432 | -0.31% | +2.92% | 4 → 5 |
| 12_binary_trees | 32,432 | +0.42% | -1.87% | 6 → 7 |
| 13_factorial | 32,448 | -0.01% | +0.21% | 104 → 112 |
| 14_closure | 32,448 | +0.18% | +1.24% | 52 → 57 |
| 15_mandelbrot | 32,448 | -0.22% | -0.74% | 27 → 26 |
| 16_matrix_multiply | 32,432 | +0.11% | +4.43% | 25 → 29 |
| 17_loop_data_dependent | 32,432 | +0.05% | +0.06% | 262 → 267 |
| bench_array_grow | 48,848 | -0.74% | -2.65% | 16 → 18 |
| bench_buffer_readwrite | 32,432 | +0.37% | +0.29% | 118 → 135 |
| bench_gc_pressure | 32,448 | -0.04% | +12.70% | 8 → 9 |
| bench_int_arithmetic | 32,432 | +0.01% | -0.95% | 39 → 33 |
| bench_json_readonly | 32,416 | -0.26% | -0.55% | 89 → 86 |
| bench_json_readonly_indexed | 32,432 | +0.00% | +0.02% | 93 → 106 |
| bench_json_roundtrip | 32,416 | -0.03% | +2.40% | 157 → 175 |
| bench_json_typed_roundtrip | 32,416 | +0.23% | +1.44% | 187 → 179 |
| bench_numeric_array_downgrade | 32,432 | -0.05% | +2.89% | 9 → 8 |
| bench_numeric_array_numeric | 32,432 | -0.12% | +3.91% | 4 → 6 |
| bench_object_property | 32,432 | -0.02% | +1.32% | 24 → 25 |
| bench_string_heavy | 32,400 | -0.04% | -7.66% | 49 → 38 |
| bench_typed_array_untyped_access | 32,368 | -0.04% | +0.09% | 609 → 625 |

The larger cycle changes on three short cases were rechecked for 31 rotated
rounds (all first-run data are retained):

| Fixture | Instructions Δ | Cycles Δ | Inner ms, reference → candidate |
| --- | ---: | ---: | ---: |
| 07_object_create | +0.20% | -0.13% | 5 → 5 |
| bench_gc_pressure | +0.04% | +0.62% | 7 → 8 |
| bench_numeric_array_numeric | -0.03% | +1.36% | 5 → 5 |

### Original application and startup

Seven rotated rounds per application workload; checksums and search result
details match throughout. Engine executable: **7,031,600 → 6,966,512 bytes**
(65,088 bytes saved). Original terminal executable: **8,680,656 → 8,615,552**
bytes. Other original application binary sizes are in the manifest.

| Workload | Runtime | Instructions G | Cycles G | Inner ms | Process s |
| --- | --- | ---: | ---: | ---: | ---: |
| search | reference | 106.90431 | 19.30005 | 9254.28 | 9.67 |
| search | cold | 107.04873 | 19.70932 | 9743.10 | 10.04 |
| evaluate | reference | 9.19017 | 1.66488 | 720.37 | 0.73 |
| evaluate | cold | 9.19179 | 1.66993 | 709.11 | 0.72 |
| legal | reference | 2.76148 | 0.47815 | 156.71 | 0.18 |
| legal | cold | 2.76687 | 0.47095 | 156.45 | 0.17 |
| perft | reference | 2.75033 | 0.46868 | 160.22 | 0.18 |
| perft | cold | 2.75187 | 0.46551 | 154.64 | 0.17 |

**Search is +5.28% inner wall time and +2.12% cycles in this run.** Its
retired instructions are +0.14%. Contention prevents attributing the difference
confidently, but it must not be dismissed as proven noise. The strict
performance-preserving acceptance condition is **not established**; retain
this change as a draft pending a quiet-host confirmation. This audit does not
close #13662 or recommend blanket Oz.

Startup uses 31 rotated pairs of `01_startup`, with wall time measured by the
parent monotonic clock (including process-launch and `time` overhead):

| Runtime | Bytes | Instructions | Cycles | Parent wall ms |
| --- | ---: | ---: | ---: | ---: |
| full-reference | 5,108,128 | 16,959,221 | 9,280,667 | 12.288 |
| full-cold | 5,075,696 | 16,945,742 | 9,607,955 | 11.786 |

### Outlining samples

Separate eight-second samples at 1 ms, after one second of startup, run
`forof 500000000` against the unstripped copies. These samples follow all
timed measurements. The profiler flat summary only lists symbols with at
least five samples; outlined shares here are therefore lower bounds.

| Runtime | Total samples | Listed outlined self samples | Share |
| --- | ---: | ---: | ---: |
| o2 | 5,999 | 0 | 0.00% |
| oz | 5,966 | 546 | 9.15% |
| cold | 6,055 | 0 | 0.00% |

O2 and the candidate each have their entire sample total accounted for by
the four listed non-outlined symbols. The Oz sample retains outlined work
in the hot scan. These are observed locations, not an estimate of entirely
removable overhead. Changes since the original issue alter the profile mix.

[Raw samples, hashes and profile summaries](../../benchmarks/runtime-size/results/2026-11-01-macos-arm64/)
include all measured runs, including the unfavorable timings.

Measurements run serially with rotated arm order, with no builds or profiling
from this experiment running concurrently. Unrelated builds continued on the
shared host. Consequently the paired instruction counts and executable sizes
are stronger evidence than wall time; these timings do not establish a strict
no-regression threshold. A quiet-host confirmation remains desirable.

Counters cover the whole process, including initialization and warmup. Inner
scan time excludes construction and 100,000 warmup iterations. Process wall
time from macOS `time -l` has coarse precision; startup is measured separately
with a monotonic parent timer, including launcher overhead. Checksums and
non-timing output must match before results are accepted. Timings are local
measurements, not a cross-platform performance guarantee.

## Correctness and coverage

- Runtime unit suite: **6,365 passed, 0 failed, 48 ignored**, serial execution,
  default features. Command: `RUST_TEST_THREADS=1 cargo test --release --locked
  --target-dir target/unit-tests --config 'profile.release.lto=false'
  --config 'profile.release.codegen-units=16' -j 2 -p perry-runtime --lib`.
  This O3 test build uses no LTO; production measurements use the O2 fat-LTO
  archives described above.
- Six compiled fixtures match Node 26.12.1 on both production runtime arms:
  `test_gap_9788_iterator_prototype_overrides`,
  `test_gap_7302_throw_across_helper_frames`,
  `test_gap_7302_gc_throw_across_collection`,
  `test_gap_descriptor_snapshot_moving`, `test_gap_typedarr_moving_owner`,
  and `test_gap_repsel_gc_stress`.
- A separate instrumented candidate build runs the descriptor-snapshot and
  typed-array-owner fixtures with schedule seed 1 and protected fromspace.
  Both match Node, and collector receipts report **302** and **7,245 moved
  objects**, respectively. This correctness-only runtime uses O1/no LTO,
  `PERRY_RUNTIME_INSTRUMENTS=1` at build time and `gc-instruments`; compiler
  and execution use `PERRY_GC_INSTRUMENTS=1`, `PERRY_GC_SCHEDULE_SEED=1`,
  `PERRY_GC_PROTECT_FROMSPACE=1`. Feature selection alone does not enable
  instrumentation in a release build. Ordinary parity runs are not counted
  as evidence of live movement. Unit tests additionally assert positive
  copying in the default-feature O3 runtime.
- Original private chess application's integration and text parity suites
  pass against Node on both production runtime arms. Terminal tests pass
  quit, Ctrl-C, SIGTERM, resize/SIGWINCH, raw-mode restoration, alternate
  screen restoration and cursor restoration. Private source is not included;
  application measurements are contextual evidence, not the public repro.
- `git diff --check` passes. The repository file-size check fails identically
  on pristine reference and candidate for six pre-existing HIR/WPFacts files:
  `walker/expr_clone.rs`, `builtins.rs`, `gen/export.rs`, `gen_rest.rs`,
  `model.rs`, `solve.rs`. This change introduces no new violation.

Coverage is macOS ARM64 only. No x86, Linux or Windows performance claim is
made. The original issue used an older source revision; its exact slowdown
ratios should not be projected onto this newer reference.

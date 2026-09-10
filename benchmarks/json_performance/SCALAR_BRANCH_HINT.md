# Scalar projection R4: branch-likelihood experiment

**Rejected for landing.** The hint restores ordinary-array dispatch order and
removes the 20 MiB repeated-read regression, but 20 MiB mixed-field access regresses
4.49% versus main with separated sample ranges. Smaller-array access medians are
also slower than R3’s earlier window. The remaining stringify differences are
unresolved. This does not meet the all-rows/no-regressions objective.

Measured implementation: `479cea228477675bcaf2213da6b93d569bd523b4` on `codex/json-scalar-projection-r4`.
Reference main: `53df2c671fff33b1f8f624432372c2401db8b1d0`, version 0.5.1530.
[R3 report](SCALAR_MATERIALIZED.md) records the preceding implementation and the
actual-main reproduction of two inherited native-root findings. No R4 PR is open.

## Change and generated-code result

R4 adds `llvm.expect.i1(is_lazy, false)` at the optional JSON brand test and declares
the intrinsic. It supplies branch-likelihood information while retaining the actual
condition and both existing outcomes. LLVM lowers this intrinsic into branch weight
metadata. [LLVM source](https://llvm.org/doxygen/LowerExpectIntrinsic_8cpp_source.html).

The installed LLVM 22.1.4 backend emitted Array → Object → JSON comparisons in the
repeat loop. R3 emitted JSON → Object → Array; main emitted Object → Array. Thus the
hint changed the actual machine dispatch, not just its source spelling.
[Machine-code evidence](results/scalar-projection-r4-validation/machine-order.json).

No runtime source, GC policy, guard, memo layout or output representation changed
relative to R3. Compiler, runtime-static and stdlib-static were rebuilt together.
The static archive hashes differ from R3; all measurements use their own frozen
artifacts and provenance, not an assumption of byte identity.

## Measurements

Quiet M1 Mac mini, 8 GiB RAM; Node 26.5.1 and Bun 1.3.14. The candidate uses seven
fresh-process trials per engine and row, interleaved with identical work counts.
CPU is user + system time per operation. Peak RSS includes process startup/input
construction. Every full-work checksum, sample vector and median was verified.

- Access: 2026-09-10T12:53:03Z–2026-09-10T12:53:26Z; 336 timed trials, 12 verification outputs; load 1.428 → 2.125, no competing workloads. [Window](results/quiet-scalar-projection-r4-access/window.json), [raw timings](results/quiet-scalar-projection-r4-access/timing.jsonl).
- Focus: 2026-09-10T12:53:29Z–2026-09-10T12:56:09Z; 280 timed trials, 50 verification outputs; load 2.114 → 1.970, no competing workloads. [Window](results/quiet-scalar-projection-r4-focus/window.json), [raw timings](results/quiet-scalar-projection-r4-focus/timing.jsonl).

### Parse, scan and stringify CPU (µs per operation)

| Fixture | Operation | Main | R4 | Node | Bun | R4 vs main | Ranges vs main |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| records_array_16k | scan | 58.307500 | 22.014750 | 40.715500 | 35.550500 | -62.24% | separated |
| records_array_1m | scan | 2900.555000 | 1367.590000 | 2778.000000 | 2200.235000 | -52.85% | separated |
| records_array_8m | scan | 23660.593750 | 11908.125000 | 31126.906250 | 21525.031250 | -49.67% | separated |
| records_array_20m | scan | 54589.875000 | 54511.312500 | 85119.937500 | 51851.625000 | -0.14% | overlap |
| records_array_20m | roundtrip | 105339.375000 | 105209.375000 | 98808.875000 | 68813.750000 | -0.12% | overlap |
| records_array_1m | parse | 917.130000 | 912.020000 | 2642.850000 | 2153.745000 | -0.56% | overlap |
| records_array_1m | stringify | 640.238281 | 642.136719 | 838.121094 | 957.359375 | +0.30% | overlap |
| small_record | parse | 0.096597 | 0.096426 | 0.339508 | 0.243425 | -0.18% | overlap |
| small_record | stringify | 0.042099 | 0.042419 | 0.107635 | 0.118091 | +0.76% | overlap |
| long_string_1m | stringify | 32.401611 | 34.298828 | 99.211182 | 92.837158 | +5.86% | overlap |

The three smaller scan rows retain roughly 50–62% CPU reductions versus main.
All their sample ranges beat both Node and Bun. The 20 MiB scan and round-trip
overlap main but still trail Bun; round-trip takes 1.53 times Bun’s CPU.
Small-record stringify (+0.76%) and long-string stringify (+5.86%) have overlapping
ranges; neither is accepted as a no-regression result or a stringify improvement.

### Parse-once access CPU (µs per iteration)

One million iterations per process, zero warmup, parse outside the timer.

| Fixture | Pattern | Main | R4 | Node | Bun | R4 vs main | Ranges vs main |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 16k | repeat | 0.018567 | 0.005327 | 0.002743 | 0.004529 | -71.31% | separated |
| 16k | random | 0.040275 | 0.019267 | 0.006670 | 0.009290 | -52.16% | separated |
| 16k | fields | 0.084510 | 0.045091 | 0.005364 | 0.009260 | -46.64% | separated |
| 16k | sequential | 0.035073 | 0.022558 | 0.003920 | 0.005757 | -35.68% | separated |
| 1m | repeat | 0.018552 | 0.005340 | 0.002749 | 0.004506 | -71.22% | separated |
| 1m | random | 0.048778 | 0.024642 | 0.010596 | 0.010744 | -49.48% | separated |
| 1m | fields | 0.107941 | 0.043009 | 0.010761 | 0.014306 | -60.16% | separated |
| 1m | sequential | 0.040639 | 0.016811 | 0.008218 | 0.007475 | -58.63% | separated |
| 20m | repeat | 0.005636 | 0.005638 | 0.003057 | 0.004792 | +0.04% | overlap |
| 20m | random | 0.026301 | 0.025772 | 0.008618 | 0.010052 | -2.01% | overlap |
| 20m | fields | 0.027184 | 0.028404 | 0.011679 | 0.014647 | +4.49% | separated |
| 20m | sequential | 0.016023 | 0.015750 | 0.006767 | 0.008235 | -1.70% | overlap |

20 MiB repeated reads now overlap main (+0.04%), addressing the specific R3
regression (+5.59%). Mixed fields move in the other direction: +4.49% versus main,
with separated ranges. All twelve access CPU rows still trail both Node and Bun.
Every smaller access median is slower than R3’s earlier window; that cross-window
observation is not a simultaneous R3/R4 comparison, but it rules out claiming an
unqualified improvement across the measured patterns.

### Peak RSS (MiB)

| Fixture | Operation/pattern | Main | R4 | Node | Bun |
| --- | --- | ---: | ---: | ---: | ---: |
| records_array_16k | scan | 216.14 | 63.08 | 61.92 | 77.38 |
| records_array_1m | scan | 413.73 | 67.22 | 130.19 | 99.59 |
| records_array_8m | scan | 530.25 | 108.91 | 313.56 | 136.03 |
| records_array_20m | scan | 691.45 | 691.53 | 529.44 | 323.42 |
| records_array_20m | roundtrip | 506.34 | 506.44 | 493.98 | 319.64 |
| records_array_1m | parse | 67.11 | 67.19 | 125.56 | 95.09 |
| records_array_1m | stringify | 63.02 | 63.12 | 130.12 | 103.69 |
| small_record | parse | 81.38 | 81.41 | 59.67 | 79.86 |
| small_record | stringify | 33.34 | 33.39 | 59.77 | 394.58 |
| long_string_1m | stringify | 54.50 | 54.55 | 255.39 | 159.77 |
| records_array_16k | repeat (reuse) | 13.05 | 13.06 | 57.80 | 34.83 |
| records_array_16k | random (reuse) | 13.42 | 13.50 | 57.75 | 35.42 |
| records_array_16k | fields (reuse) | 13.17 | 13.27 | 57.92 | 35.98 |
| records_array_16k | sequential (reuse) | 13.17 | 13.08 | 57.77 | 35.25 |
| records_array_1m | repeat (reuse) | 17.58 | 17.61 | 63.58 | 37.94 |
| records_array_1m | random (reuse) | 18.47 | 18.55 | 66.50 | 38.91 |
| records_array_1m | fields (reuse) | 18.27 | 18.36 | 66.69 | 40.31 |
| records_array_1m | sequential (reuse) | 18.27 | 17.62 | 66.56 | 39.22 |
| records_array_20m | repeat (reuse) | 96.91 | 97.03 | 194.75 | 93.95 |
| records_array_20m | random (reuse) | 96.92 | 97.03 | 194.84 | 94.75 |
| records_array_20m | fields (reuse) | 96.94 | 97.03 | 195.05 | 95.34 |
| records_array_20m | sequential (reuse) | 96.94 | 97.03 | 194.84 | 94.61 |

Scan peak RSS remains about 67 MiB at 1 MiB and 109 MiB at 8 MiB, versus main’s
414 and 530 MiB. The 16 KiB scan still slightly exceeds Node. 20 MiB scan RSS
remains 692 MiB versus Bun’s 323 MiB. Small absolute increases in other rows remain
visible in the table. Retained-RAM and rotating-input matrices were not rerun,
because the targeted CPU comparison already rejects this candidate.

## Tiny-stringify crossover

This separate diagnostic investigates the R3 regression. It times **R3**, not R4,
against main and a crossover consisting of main’s existing worker object linked
with the frozen R3 runtime. The existing imported function ABI and eager
Object/Array layouts remain compatible; the binary is used only for this diagnostic.
Normal, scheduled-moving and full-GC outputs and full checksums match Node. The
scheduled diagnostic protects 106 retired sets and moves 12,720 objects.

The qualified window is 2026-09-10T12:57:37Z–2026-09-10T12:58:11Z, load 1.898 → 1.825, no competing workloads. Nine trials per engine (45 total), ten million iterations and 5,000 warmup operations each; six verification outputs. [Window](results/quiet-scalar-projection-r4-tiny-crossover/window.json), [engine mapping](results/quiet-scalar-projection-r4-tiny-crossover/diagnostic.json), [provenance](results/quiet-scalar-projection-r4-tiny-crossover/hybrid-provenance.json).

| Engine/build | Median CPU µs/op | Sample range | Peak RSS MiB |
| --- | ---: | --- | ---: |
| Main compiler + main runtime | 0.0421409 | 0.0421054–0.0437169 | 33.34 |
| Main compiler + R3 runtime | 0.0423118 | 0.0422255–0.0426962 | 33.34 |
| R3 compiler + R3 runtime | 0.0424487 | 0.0423588–0.0427675 | 33.41 |
| Node | 0.1077714 | 0.1073607–0.1088084 | 59.77 |
| Bun | 0.1184765 | 0.1178235–0.1190026 | 394.58 |

R3’s median is +0.73% versus main; the crossover is +0.41% versus main; R3 is
+0.32% versus the crossover. **All three ranges overlap.** This does not isolate
one cause, establish a runtime/codegen split, or clear the stringify regression.
All three Perry binary hashes and all 45 timed checksums were independently verified.

## Validation

- Exact three-package release build completed in 6m06s; frozen compiler and both archive hashes still match after unit tests.
- 301 runtime JSON and 10 codegen JSON tests pass; 13 index-get, 25 property-get and 15 GC-effect selections pass.
- All 31 fixture lines match Node in nine auto/tape/direct × normal/scheduled/full-GC modes. Each scheduled arm protects 776 retired sets; auto/tape moves 33,614 objects and direct moves 88,384.
- Nine mutation and twelve access controls pass. Descriptor/getter, forwarding, prototype, identity, growth/shrink and coercion checks remain in the fixture.
- Native benchmark workers: 18 functions, 582 statepoints, 1,239 relocates, zero hazards. Full native fixture: the same two inherited unsuppressed findings reproduced on actual main during R3; both full shadow checks pass.
- Every emitted scalar probe has a matching expectation intrinsic: 54 fixture, 12 access-worker and 4 original-worker sites under both root lowerings. Neither changes the noncollecting probe contract.
- Script gates: 73/74 pass; public benchmark freshness still fails. Compile lint tier skipped. No complete CI or JSON-conformance pass is claimed.

[Validation manifest](results/scalar-projection-r4-validation/manifest.json),
[build provenance](results/scalar-projection-r4-validation/build-provenance.json),
[GC witness](results/scalar-projection-r4-validation/gc-witness.json),
[completion record](results/scalar-projection-r4-validation/completion.json).

## Next investigation

Reordering the per-access branches produced a tradeoff. The existing
`element_shape_loop` machinery instead validates an array once before a call-free
loop, keeping necessary residual checks. Its matcher requires a declared class
candidate, raw-f64 fields and the counter itself as the index. The runtime proof
explicitly rejects class ID zero. Consequently these untyped JSON records and
constant/modulo index patterns do not receive that optimization.

Investigate a distinct, validated numeric-data-record loop contract. A bounded
first slice is a loop-invariant indexed numeric read: establish a side-effect-free
numeric value once, then use it in a call-free reduction. Zero-trip loops must not
touch a potentially throwing receiver, and failed probes must never invoke a getter
before falling back. Varying-index and mixed-field loops need their own precise
guard and side-exit proof. Do not weaken the existing typed-class invariant or
change collector policy. [Detailed constraints and code pointers](results/scalar-projection-r4-validation/next-loop.md).

The overall objective still includes parse, stringify, all access controls,
changing inputs, peak RSS and retained RAM. The full original matrix was not rerun
for this rejected candidate. Large-input round-trip/RSS and inherited JSON
canonicalization/conformance limitations remain separate outstanding work.

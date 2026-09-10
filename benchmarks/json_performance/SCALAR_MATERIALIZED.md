# Scalar projection R3: materialized-array dispatch

**Rejected for landing.** R3 fixes the targeted smaller-array random and mixed-field
access regressions, and retains the scalar-scan CPU/RSS wins. Three measured CPU
regressions remain: 20 MiB repeated reads (+5.59%), 20 MiB mixed fields (+0.97%),
and small-record stringify (+0.66%). Each has nonoverlapping seven-sample ranges
against main. The goal of matching Node and Bun on every row is not achieved.

Measured implementation: `2647ea7f0192283e6c140e0576d0b7f62419a33d` on `codex/json-scalar-projection-r3`.
Reference: actual main `53df2c671fff33b1f8f624432372c2401db8b1d0`, version 0.5.1530.
R2 remains documented in [SCALAR_MEMO.md](SCALAR_MEMO.md). No PR is open for R3.

## Change and correctness contract

After scalar access has given way to full materialization, R2 still sends indexed
reads through the lazy-array runtime dispatcher. R3 validates the materialized
backing array and feeds it into the existing ordinary-array guards. The original
boxed receiver remains the fallback for forwarding, descriptors and exotic cases.

The fast path verifies lazy brand/magic/descriptor state, the backing Array brand
and its forwarding bit. It refreshes the lazy wrapper’s cached length, matching
the runtime behavior after mutation through another alias. The shared guard reads
flags, length, capacity and elements from the selected backing array. Existing
prototype invalidation, bounds, holes, getters and property lookup remain active.
The new length store is pointer-free. No allocation, managed cache, runtime producer
or GC policy is added relative to R2.

The source keeps JSON after ordinary Array/Object branches, but optimized machine
code does not preserve that order; see the next investigation below.

## Measurements

Quiet M1 Mac mini, 8 GiB, Node 26.5.1, Bun 1.3.14. Seven fresh processes per engine
and row, interleaved with identical work counts. CPU is user + system time per
operation. RSS is the full-process peak, including startup and input construction.
These are 22 targeted CPU rows, not a replacement full 38-row parse/stringify matrix.

- Access: 2026-09-10T12:30:02Z–2026-09-10T12:30:25Z; 336 timed trials, 12 verification outputs. Load 1.570 → 1.971; no competing workloads. [Window](results/quiet-scalar-projection-r3-access/window.json), [raw timings](results/quiet-scalar-projection-r3-access/timing.jsonl).
- Focus: 2026-09-10T12:30:28Z–2026-09-10T12:33:08Z; 280 timed trials, 50 verification outputs. Load 1.971 → 1.659; no competing workloads. [Window](results/quiet-scalar-projection-r3-focus/window.json), [raw timings](results/quiet-scalar-projection-r3-focus/timing.jsonl).

All 616 timed checksums match their complete Node workload oracle. Every median
and sample vector below was independently recomputed from the archived raw trials.

### Parse, scan and stringify CPU (µs per operation)

| Fixture | Operation | Main | R3 | Node | Bun | R3 vs main | Ranges vs main |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| records_array_16k | scan | 58.394500 | 22.038250 | 40.081750 | 35.532250 | -62.26% | separated |
| records_array_1m | scan | 2899.835000 | 1367.705000 | 2749.585000 | 2201.955000 | -52.84% | separated |
| records_array_8m | scan | 23678.593750 | 11907.656250 | 31182.875000 | 21488.937500 | -49.71% | separated |
| records_array_20m | scan | 54463.125000 | 54452.375000 | 86957.687500 | 52403.562500 | -0.02% | overlap |
| records_array_20m | roundtrip | 104966.875000 | 104944.000000 | 99389.875000 | 68736.000000 | -0.02% | overlap |
| records_array_1m | parse | 914.710000 | 916.420000 | 2645.750000 | 2152.165000 | +0.19% | overlap |
| records_array_1m | stringify | 639.871094 | 639.882812 | 840.011719 | 957.195312 | +0.00% | overlap |
| small_record | parse | 0.096590 | 0.096729 | 0.343332 | 0.243501 | +0.14% | overlap |
| small_record | stringify | 0.042151 | 0.042428 | 0.107791 | 0.117962 | +0.66% | separated |
| long_string_1m | stringify | 33.687256 | 33.308594 | 99.118652 | 92.784424 | -1.12% | overlap |

All samples for the 16 KiB, 1 MiB and 8 MiB scans beat both Node and Bun.
The 20 MiB scan and round-trip now overlap main, but still trail Bun. Round-trip
uses 1.53 times Bun’s CPU. The small-record stringify regression is unresolved.
Long-string stringify overlaps main; this run establishes no stringify speedup.

### Parse-once access CPU (µs per iteration)

Parse occurs before the timed region; each trial executes one million iterations
with zero warmup. These expose access costs hidden by parse-only benchmarks.

| Fixture | Pattern | Main | R3 | Node | Bun | R3 vs main | Ranges vs main |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| 16k | repeat | 0.018570 | 0.005014 | 0.002744 | 0.004523 | -73.00% | separated |
| 16k | random | 0.040471 | 0.018914 | 0.006656 | 0.009302 | -53.27% | separated |
| 16k | fields | 0.084467 | 0.041379 | 0.005328 | 0.009247 | -51.01% | separated |
| 16k | sequential | 0.035077 | 0.021023 | 0.003929 | 0.005749 | -40.07% | separated |
| 1m | repeat | 0.018557 | 0.005021 | 0.002751 | 0.004557 | -72.94% | separated |
| 1m | random | 0.048773 | 0.023414 | 0.010591 | 0.010759 | -51.99% | separated |
| 1m | fields | 0.107928 | 0.040606 | 0.010767 | 0.014310 | -62.38% | separated |
| 1m | sequential | 0.040595 | 0.015645 | 0.008263 | 0.007504 | -61.46% | separated |
| 20m | repeat | 0.005636 | 0.005951 | 0.003047 | 0.004799 | +5.59% | separated |
| 20m | random | 0.026284 | 0.025673 | 0.008613 | 0.010044 | -2.32% | overlap |
| 20m | fields | 0.027182 | 0.027445 | 0.011755 | 0.014552 | +0.97% | separated |
| 20m | sequential | 0.016018 | 0.015546 | 0.006832 | 0.008278 | -2.95% | separated |

All eight 16 KiB/1 MiB access rows improve versus main with separated ranges.
Random access improves 53.3% and 52.0%, respectively; 1 MiB mixed fields improve
62.4%. Every access CPU row still trails both Node and Bun. The two 20 MiB access
regressions prevent acceptance even though their absolute per-iteration deltas are small.

### Peak RSS (MiB)

| Fixture | Operation/pattern | Main | R3 | Node | Bun |
| --- | --- | ---: | ---: | ---: | ---: |
| records_array_16k | scan | 216.14 | 63.08 | 61.94 | 77.38 |
| records_array_1m | scan | 413.72 | 67.22 | 130.23 | 99.58 |
| records_array_8m | scan | 530.25 | 108.91 | 313.64 | 139.42 |
| records_array_20m | scan | 691.44 | 691.53 | 529.34 | 293.64 |
| records_array_20m | roundtrip | 506.34 | 506.42 | 494.05 | 318.92 |
| records_array_1m | parse | 67.11 | 67.19 | 125.61 | 95.08 |
| records_array_1m | stringify | 63.02 | 63.11 | 130.08 | 103.69 |
| small_record | parse | 81.34 | 81.42 | 59.61 | 79.86 |
| small_record | stringify | 33.34 | 33.41 | 59.73 | 394.58 |
| long_string_1m | stringify | 54.48 | 54.55 | 255.45 | 153.78 |
| records_array_16k | repeat (reuse) | 13.05 | 13.05 | 57.77 | 34.80 |
| records_array_16k | random (reuse) | 13.42 | 13.50 | 57.81 | 35.42 |
| records_array_16k | fields (reuse) | 13.17 | 13.27 | 57.97 | 35.98 |
| records_array_16k | sequential (reuse) | 13.17 | 13.06 | 57.73 | 35.27 |
| records_array_1m | repeat (reuse) | 17.56 | 17.59 | 63.66 | 37.92 |
| records_array_1m | random (reuse) | 18.48 | 18.56 | 66.58 | 38.91 |
| records_array_1m | fields (reuse) | 18.28 | 18.38 | 66.73 | 40.31 |
| records_array_1m | sequential (reuse) | 18.27 | 17.59 | 66.59 | 39.20 |
| records_array_20m | repeat (reuse) | 96.91 | 97.08 | 194.70 | 93.97 |
| records_array_20m | random (reuse) | 96.92 | 97.08 | 194.78 | 94.78 |
| records_array_20m | fields (reuse) | 96.94 | 97.09 | 195.00 | 95.33 |
| records_array_20m | sequential (reuse) | 96.94 | 97.09 | 194.84 | 94.62 |

At 1 MiB, scan peak RSS falls from 413.72 to 67.22 MiB; at 8 MiB, from
530.25 to 108.91 MiB. The 16 KiB scan still uses slightly more RSS than Node.
20 MiB scan RSS remains 691.53 MiB versus Bun’s 293.64 MiB in this window.
The small absolute RSS increases in many controls remain recorded, not rounded
into a claim of no regression. Retained-RAM and rotating-input matrices were not
rerun because the targeted CPU gate already rejects this candidate.

## Validation and limits

- Rebuilt compiler, runtime-static and stdlib-static together from the frozen source. All three hashes still match after unit tests.
- 301 runtime JSON and 10 codegen JSON tests passed; 13 index-get, 25 property-get and 15 GC-effect selections also passed.
- Compiled fixture: all 31 output lines match Node in nine combinations of auto/tape/direct parsing and normal/scheduled/full GC. The alias test covers growth, identity, prototype holes and shrink.
- Each scheduled arm protected 776 retired sets; auto/tape moved 33,614 objects and direct moved 88,384. Nine mutation controls and twelve access controls also match Node.
- Native benchmark workers: 18 functions, 582 statepoints, 1,239 relocates, zero hazards. The full three-module fixture check retains exactly two findings. Both shadow checks pass.
- Probe liveness: 54 scalar calls in the strengthened fixture, 12 in the access worker and 4 in the original worker under each lowering. The helper is never a statepoint callee.
- Script gates: 73/74 pass after a standalone peer-fallback rerun. An accidentally overlapping lint invocation interfered with that test’s temporary fixture; its rerun passed. Public benchmark freshness still fails; the compile lint tier was skipped. No full CI pass is claimed.

[Validation manifest](results/scalar-projection-r3-validation/manifest.json),
[compiled outputs and moving-GC witness](results/scalar-projection-r3-validation/gc-witness.json),
[build provenance](results/scalar-projection-r3-validation/build-provenance.json),
[source/guard audit](results/scalar-projection-r3-validation/audit.md).

### Actual-main reproduction of native findings

Main was rebuilt from clean source with the same three-package release command.
Its compiler and archives were frozen and hashed. The exact saved 28-line R2
fixture was compiled separately with actual R2 and actual main compilers, then
checked using identical saved checker bytes and production LLVM passes.

Both produce exactly the same two fingerprints: a global string value across
`js_get_string_pointer_unified`, and an allocated value across
`js_closure_unbox_callee_checked`. Each has 790 statepoints, 601 live bundles and
3,559 relocates. Main emits zero scalar probes; R2 emits 49. R3’s strengthened
fixture retains those same finding shapes and adds no others.

This establishes that the findings predate scalar projection. They remain
unsuppressed and unresolved; it is not a clean native-checker verdict.
[Main/R2 comparison](results/scalar-projection-r3-validation/native-main-proof/comparison.json),
[main build provenance](results/scalar-projection-r3-validation/native-main-proof/main-build-provenance.json).

## Next investigation

The 20 MiB input exceeds the 16 MiB lazy cap and uses ordinary Arrays. Disassembly
of the frozen access objects reveals a concrete extra cost: main tests Object
and Array brands, while LLVM places R3’s LazyArray comparison before both.
The shared backing-array path also introduces a register move. Source dispatch
order therefore does not prove that the ordinary machine path is unchanged.

Investigate a dispatch form that preserves the ordinary hot path after LLVM
optimization, with the existing guards intact. Remeasure the same controls and
smaller-array wins before attributing the entire timing delta to this change.
The tiny stringify regression also needs an independent comparison of the
emitted worker/runtime paths. No collector-policy change is needed for either
investigation. [Disassembly notes](results/scalar-projection-r3-validation/next-dispatch.md).

Large-input lazy parsing remains a separate bounded experiment: forcing the
lazy path may help scalar scans, but must also be measured with mixed-field and
full-object consumption before changing the 16 MiB cap. Inherited JSON
canonicalization/conformance limitations remain; this is not full JSON conformance proof.

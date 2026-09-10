# Outlined JSON stringify entry (R7)

**Rejected for landing.** Outlining shrinks the public entry frame from 272 to 160 bytes, but does not provide a consistent speedup. Against R5, short-string stringify regresses **3.24%**, numeric-zero spacing **0.71%**, and key-list replacement **0.91%**, with separated sample ranges. Empty-object and tiny-object stringify improve **1.06%** and **0.59%**, also with separated ranges. These small gains do not compensate for the regressions.

Against frozen main, small-record stringify is **1.48% slower** and 1k-object stringify **3.56% slower**, both with separated ranges. Their R5 comparisons overlap and do not establish an isolated outlining regression. The inherited 20m mixed-field regression remains **3.06%**, essentially identical to R5. This branch is not a merge candidate; no PR was opened for R7.

Source: `6a949843ffab888596b47d643fb4ea1c59ebf156` on `codex/json-stringify-entry-r7`, based on R5 and excluding R6. Baseline is frozen main `53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530), not a fresh measurement of the latest main. Five engines run interleaved on the M1 Mac mini (8 GiB), seven fresh-process trials per row, Node 26.5.1 and Bun 1.3.14.

CPU is microseconds per operation. Access rows count one loop iteration after parsing; focus and options rows count the complete operation. Options input is eagerly parsed before timing. Peak RSS is MiB for the whole process, including input, runtime, output, and allocator storage; it is not retained heap. Negative deltas are faster. Every sample, including outliers, is retained.

## Access after parsing

Window: 2026-09-10T15:32:26Z to 2026-09-10T15:32:54Z; one-minute load 1.477 to 1.750. Quiet gate passed, no detected competing workloads at either boundary.

| Workload | Main | R5 | R7 | Node | Bun | R7 vs main | R7 vs R5 |
|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k / repeat | 0.018546 | 0.000946 | 0.000943 | 0.002743 | 0.004538 | -94.92% | -0.32% |
| records_array_16k / random | 0.040145 | 0.018900 | 0.018901 | 0.006656 | 0.009280 | -52.92% | +0.01% |
| records_array_16k / fields | 0.084457 | 0.043048 | 0.043017 | 0.005353 | 0.009268 | -49.07% | -0.07% |
| records_array_16k / sequential | 0.035059 | 0.022051 | 0.022064 | 0.003920 | 0.005728 | -37.07% | +0.06% |
| records_array_1m / repeat | 0.018562 | 0.000946 | 0.000947 | 0.002748 | 0.004559 | -94.90% | +0.11% |
| records_array_1m / random | 0.048811 | 0.023426 | 0.023670 | 0.010629 | 0.010761 | -51.51% | +1.04% |
| records_array_1m / fields | 0.107940 | 0.041227 | 0.041175 | 0.010811 | 0.014331 | -61.85% | -0.13% |
| records_array_1m / sequential | 0.040603 | 0.016198 | 0.016177 | 0.008151 | 0.007465 | -60.16% | -0.13% |
| records_array_20m / repeat | 0.005643 | 0.000953 | 0.000947 | 0.003019 | 0.004805 | -83.22% | -0.63% |
| records_array_20m / random | 0.026285 | 0.025425 | 0.025617 | 0.008561 | 0.010042 | -2.54% | +0.76% |
| records_array_20m / fields | 0.027211 | 0.028045 | 0.028045 | 0.011734 | 0.014601 | +3.06% | +0.00% |
| records_array_20m / sequential | 0.016383 | 0.015644 | 0.015708 | 0.006870 | 0.008213 | -4.12% | +0.41% |

Peak RSS:

| Workload | Main | R5 | R7 | Node | Bun |
|---|---:|---:|---:|---:|---:|
| records_array_16k / repeat | 13.05 | 13.03 | 13.11 | 57.77 | 34.81 |
| records_array_16k / random | 13.44 | 13.47 | 13.50 | 57.83 | 35.42 |
| records_array_16k / fields | 13.17 | 13.23 | 13.27 | 58.00 | 36.00 |
| records_array_16k / sequential | 13.17 | 13.05 | 13.14 | 57.77 | 35.23 |
| records_array_1m / repeat | 17.56 | 17.58 | 17.64 | 63.77 | 37.92 |
| records_array_1m / random | 18.48 | 18.53 | 18.56 | 66.59 | 38.92 |
| records_array_1m / fields | 18.27 | 18.34 | 18.36 | 66.66 | 40.31 |
| records_array_1m / sequential | 18.28 | 17.59 | 17.67 | 66.58 | 39.20 |
| records_array_20m / repeat | 96.92 | 97.02 | 97.05 | 194.75 | 93.98 |
| records_array_20m / random | 96.92 | 97.02 | 97.05 | 194.80 | 94.72 |
| records_array_20m / fields | 96.92 | 97.00 | 97.05 | 195.02 | 95.36 |
| records_array_20m / sequential | 96.92 | 97.02 | 97.05 | 194.86 | 94.56 |

[Raw samples](results/quiet-stringify-entry-r7-access/timing.jsonl), [complete-output verification](results/quiet-stringify-entry-r7-access/verify.jsonl), [CPU/RSS vectors and medians](results/quiet-stringify-entry-r7-access/summary.json), [qualified window](results/quiet-stringify-entry-r7-access/window.json).

## Parse, stringify, and consumption

Window: 2026-09-10T15:32:57Z to 2026-09-10T15:37:05Z; one-minute load 1.750 to 2.390. Quiet gate passed, no detected competing workloads at either boundary.

| Workload | Main | R5 | R7 | Node | Bun | R7 vs main | R7 vs R5 |
|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k / scan | 58.667000 | 22.138750 | 22.151750 | 41.025250 | 35.680250 | -62.24% | +0.06% |
| records_array_1m / scan | 2920.510000 | 1369.300000 | 1370.315000 | 2669.950000 | 2204.405000 | -53.08% | +0.07% |
| records_array_8m / scan | 23844.437500 | 11919.687500 | 11907.437500 | 31548.312500 | 21366.281250 | -50.06% | -0.10% |
| records_array_20m / scan | 54747.687500 | 54730.750000 | 54570.062500 | 84808.562500 | 52423.000000 | -0.32% | -0.29% |
| records_array_20m / roundtrip | 105504.375000 | 105470.375000 | 105302.750000 | 99373.875000 | 68900.500000 | -0.19% | -0.16% |
| records_array_1m / parse | 914.235000 | 915.575000 | 915.345000 | 2630.160000 | 2149.695000 | +0.12% | -0.03% |
| records_array_1m / stringify | 640.882812 | 640.011719 | 641.851562 | 840.664062 | 956.480469 | +0.15% | +0.29% |
| small_record / parse | 0.096588 | 0.096744 | 0.096663 | 0.353276 | 0.244812 | +0.08% | -0.08% |
| small_record / stringify | 0.042263 | 0.042415 | 0.042890 | 0.107925 | 0.118467 | +1.48% | +1.12% |
| long_string_1m / stringify | 33.220947 | 33.208008 | 33.663330 | 98.977051 | 92.836670 | +1.33% | +1.37% |
| null / stringify | 0.006566 | 0.006569 | 0.006566 | 0.026323 | 0.029751 | +0.00% | -0.05% |
| string_a / stringify | 0.009696 | 0.009695 | 0.010010 | 0.029168 | 0.030581 | +3.23% | +3.24% |
| empty_object / stringify | 0.017633 | 0.017580 | 0.017394 | 0.031146 | 0.030774 | -1.36% | -1.06% |
| tiny_object / stringify | 0.030443 | 0.030492 | 0.030311 | 0.036363 | 0.040799 | -0.43% | -0.59% |
| object_1k / stringify | 0.111819 | 0.113363 | 0.115799 | 0.190006 | 0.207452 | +3.56% | +2.15% |

Peak RSS:

| Workload | Main | R5 | R7 | Node | Bun |
|---|---:|---:|---:|---:|---:|
| records_array_16k / scan | 216.14 | 63.08 | 63.06 | 61.94 | 77.38 |
| records_array_1m / scan | 413.73 | 67.23 | 67.20 | 130.19 | 99.58 |
| records_array_8m / scan | 530.25 | 108.91 | 108.89 | 313.58 | 136.42 |
| records_array_20m / scan | 691.44 | 691.53 | 691.50 | 529.22 | 293.95 |
| records_array_20m / roundtrip | 506.34 | 506.72 | 506.42 | 493.95 | 319.58 |
| records_array_1m / parse | 67.11 | 67.19 | 67.17 | 125.52 | 95.00 |
| records_array_1m / stringify | 63.02 | 63.11 | 63.11 | 130.08 | 103.70 |
| small_record / parse | 81.38 | 81.42 | 81.42 | 59.62 | 79.89 |
| small_record / stringify | 33.34 | 33.41 | 33.38 | 59.80 | 394.58 |
| long_string_1m / stringify | 54.48 | 54.55 | 54.52 | 255.39 | 151.67 |
| null / stringify | 13.02 | 13.05 | 13.12 | 59.64 | 134.17 |
| string_a / stringify | 13.02 | 13.05 | 13.12 | 59.61 | 134.14 |
| empty_object / stringify | 13.11 | 13.16 | 13.23 | 59.64 | 134.17 |
| tiny_object / stringify | 32.33 | 32.39 | 32.36 | 59.70 | 134.17 |
| object_1k / stringify | 33.38 | 33.42 | 33.39 | 61.80 | 70.91 |

[Raw samples](results/quiet-stringify-entry-r7-focus/timing.jsonl), [complete-output verification](results/quiet-stringify-entry-r7-focus/verify.jsonl), [CPU/RSS vectors and medians](results/quiet-stringify-entry-r7-focus/summary.json), [qualified window](results/quiet-stringify-entry-r7-focus/window.json).

## Stringify fallback arguments

Window: 2026-09-10T15:37:08Z to 2026-09-10T15:37:37Z; one-minute load 2.390 to 1.841. Quiet gate passed, no detected competing workloads at either boundary.

| Workload | Main | R5 | R7 | Node | Bun | R7 vs main | R7 vs R5 |
|---|---:|---:|---:|---:|---:|---:|---:|
| small_record / zero | 0.440819 | 0.442508 | 0.445640 | 0.248249 | 0.396306 | +1.09% | +0.71% |
| small_record / pretty | 0.513890 | 0.512450 | 0.511635 | 0.308555 | 0.540435 | -0.44% | -0.16% |
| small_record / keys | 0.469560 | 0.469655 | 0.473950 | 0.597625 | 0.499600 | +0.93% | +0.91% |
| small_record / callback | 1.031620 | 1.031100 | 1.033600 | 0.615180 | 0.565940 | +0.19% | +0.24% |
| records_array_16k / pretty | 45.806000 | 45.542000 | 45.552000 | 34.796000 | 46.034000 | -0.55% | +0.02% |

Peak RSS:

| Workload | Main | R5 | R7 | Node | Bun |
|---|---:|---:|---:|---:|---:|
| small_record / zero | 33.69 | 33.72 | 33.84 | 59.55 | 207.69 |
| small_record / pretty | 33.50 | 33.50 | 33.64 | 59.56 | 77.61 |
| small_record / keys | 32.66 | 32.67 | 32.78 | 59.52 | 71.12 |
| small_record / callback | 27.53 | 27.52 | 27.66 | 59.69 | 51.19 |
| records_array_16k / pretty | 24.30 | 24.31 | 24.44 | 55.64 | 39.69 |

[Raw samples](results/quiet-stringify-entry-r7-options/timing.jsonl), [complete-output verification](results/quiet-stringify-entry-r7-options/verify.jsonl), [CPU/RSS vectors and medians](results/quiet-stringify-entry-r7-options/summary.json), [qualified window](results/quiet-stringify-entry-r7-options/window.json).

## Implementation and evidence

The four existing bounded-output attempts remain in their original order in `js_json_stringify_full`; the original general fallback body moves into a non-inlined internal Rust helper. The public ABI and all three argument bits are preserved. The only fallback-body substitution is receiving the original inert-argument predicate as `plain`; [source equality proof](results/stringify-entry-r7-validation/source-transformation.json) records this. No new GC boundary, policy, representation, or managed-pointer cache is introduced.

The [linked ARM64 disassembly](results/stringify-entry-r7-validation/candidate-stringify-full-machine.s.gz) shows a 160-byte public entry frame, restored before a tail branch to the 304-byte fallback frame. R5's public entry uses 272 bytes. The frames are not simultaneously stacked by that transition. All three standard generated worker objects are byte-identical to R5, as is the options-worker object ([standard object hashes](results/stringify-entry-r7-validation/entry-machine.json), [options object hashes](results/stringify-entry-r7-validation/options-object-comparison.json)). The smaller frame is a verified code change; these measurements reject it as a general performance improvement.

The exact three-package production build completed in 330.56 seconds with all artifacts newer than build start, a clean unchanged source tree, and frozen compiler/runtime/stdlib hashes. [Build provenance](results/stringify-entry-r7-validation/build-provenance.json) and [worker build/link commands](results/stringify-entry-r7-validation/provenance.json) identify the actual binaries; target-directory contents were not assumed to match the checkout.

The 20m scan still peaks at **691.50 MiB**, versus Bun's **293.95 MiB** in this window; roundtrip peaks at **506.42 MiB**, versus **319.58 MiB**. This change does not resolve the large-input memory gap. Option paths add roughly **0.11–0.14 MiB** to R5's whole-process RSS medians. The entry frame reduction does not yield a meaningful process-memory reduction.

A useful follow-up is inert spacer admission: the zero-spacing control takes **0.445640 µs**, versus **0.042890 µs** for the ordinary small-record stringify control, about **10.4×** as long. Those are separate workers and windows, so this is a lead, not an isolated causal A/B. The existing entry admits null/undefined/false spacers but sends numeric zero through the general path. Test a guarded admission change—or normalization of a provably inert literal argument—on a fresh main base, with the other argument forms and ordinary entry costs kept as controls. Boxed spacer coercion must remain observable. No such change is part of R7.

## Validation and limits

The final source passes 306 runtime JSON tests and 10 codegen JSON tests with `RUST_TEST_THREADS=1`. The 12-case new runtime unit covers inert arguments, indentation, ordered/deduplicated/empty key lists, arrays, and primitive/string output. The new TypeScript fixture covers callback ordering, reentrant stringify, root dropping, exceptions, cycles, boxed spacer coercion, and live input/output retention under allocation pressure.

All 27 parser/GC combinations (three fixtures × auto/tape/direct × normal/scheduled/full mark-sweep) match Node. Every scheduled run asserts positive moved-object and protected-from-space counts. The entry fixture protects 1,208 retired sets and moves 46,150/46,228/46,150 objects for auto/tape/direct. All nine mutation and 12 access controls pass. Five existing stringify regression fixtures match Node on both frozen R5 and R7. Each of five options controls matches Node in normal and scheduled runs on main/R5/R7; every scheduled arm protects six retired sets.

The callback-only probe has its only JavaScript loop inside the replacer. Both R5 and R7 match Node while protecting 16 retired sets and moving 13,305 objects. Its candidate native IR has zero hazards across nine functions, 71 statepoints, and 91 relocates. This is a live collection during the serializer's runtime frame, not just collection between completed calls.

Both whole-corpus shadow-root variants pass. Native ordinary workers have zero hazards across 18 functions, 582 statepoints, and 1,239 relocates. The full native fixture corpus reports eight **unsuppressed** findings (seven unrooted and one stale); every complete fingerprint matches its actual frozen main proof. The options worker reports two further unsuppressed global-to-singleton-allocation findings, both reproduced with frozen R5. All are classified MOVING=no by the checker. Baseline equality is not a clean static verdict or a justification to suppress them. No allowlist was added, and the stale budget remains zero in the final comparison.

The script lint run passes 73 of 74 checks; the public benchmark evidence freshness gate fails. The compile tier and two CI-only checks were skipped. The Rust file-size cap passes (`replacer.rs`: 1,994 lines). This is not a full-CI pass.

Two parsed-empty-array conformance gaps were reproduced on actual frozen main, R5, and R7. A parsed `[]` used as replacer is ignored, while a literal empty list filters out all keys. Pretty-printing a parsed empty input array returns an empty string, while the literal empty array returns `[]`. The new unit uses the working literal-array representation for its empty-array controls. Initial failing unit logs, final source hashes, Node output, and baseline/candidate probes are retained; neither inherited gap is claimed fixed by outlining.

[Unit log](results/stringify-entry-r7-validation/unit.log.gz), [GC witnesses](results/stringify-entry-r7-validation/gc-witness.json), [callback proof](results/stringify-entry-r7-validation/candidate-callback-proof.json), [complete root fingerprints](results/stringify-entry-r7-validation/root-fingerprint-comparison.json), [inherited conformance gaps](results/stringify-entry-r7-validation/candidate-inherited-gaps.json), and [validation archive manifest](results/stringify-entry-r7-validation/manifest.json).

All 1120 timed checksums, CPU/RSS sample vectors, medians, source-patch hashes, and quiet windows were independently checked by [analyze.py](results/stringify-entry-r7-validation/analyze.py).

The full original 38-row matrix, rotating-source suite, short-call controls, and retained-memory suite were not rerun for this experiment. Earlier results remain in [merged-main measurements](MERGED_MAIN_53DF.md) and [R5](INVARIANT_FIELD_LOOP.md). The no-regressions objective remains open.

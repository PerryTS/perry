# Guarded invariant JSON field loops (R5)

Repeated own-number reads now take **0.946–0.953 ns per iteration** on the quiet M1 host: **2.9–3.2× faster than Node** and **4.8–5.1× faster than Bun** for the 16k, 1m, and 20m access controls. All seven candidate samples beat both peer ranges for 1m and 20m. The 16k first candidate trial is 3.234 ns and trails Node; the other six beat both peers. Every sample is retained. The ARM64 reduction body contains only `fadd`, `subs`, and the back-edge branch.

**Not approved for landing.** The 20m mixed-field control regresses **3.06%** against frozen main, and small-record stringify regresses **0.67%**; both sample ranges are separated. The one-iteration reduction also costs **49.9% more than R3 by median**, although its full sample range overlaps R3 because the first trial of every Perry binary was elevated. These samples are retained. No cause is assigned to that first-trial effect.

Source: `24a8ab6953ba811f5112e49ba1c79ae6e683118b`, on `codex/json-invariant-field-loop-r5`, based on [R3](SCALAR_MATERIALIZED.md). The R4 branch hint is deliberately excluded. The interleaved baseline is actual frozen main `53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530). Main subsequently advanced to `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531, Geisterhand linking changes); this is not a measurement of that newer build. R3 short-loop references use its actual frozen compiler/runtime build from `2647ea7f0192283e6c140e0576d0b7f62419a33d`.

The [compiler guard](../../crates/perry-codegen/src/stmt/invariant_field_loop.rs) recognizes a single reduction such as `sum += rows[7].id`. It proves a positive, integral, bounded trip count before inspecting the receiver. A [non-collecting probe](../../crates/perry-runtime/src/json_tape/scalar_projection.rs) accepts an own numeric data field from a supported dense or lazy array, including exposed or materialized JSON records. Missing fields, holes, descriptors, declared classes, forwarding headers, and non-numbers decline without getters or coercions. The fast loop uses private scalar slots, preserves sequential floating-point additions, and writes back at exit. Its entered blocks are certified free of GC-unsafe calls. R5 adds no GC policy, object layout, or managed-pointer cache changes relative to R3.

CPU medians below are **µs per operation**. Access rows time a loop iteration after parsing once; “fields” performs the existing three-field workload. Focus rows include their existing parse/stringify/scan work. Each uses seven fresh-process trials, interleaved with main, Node 26.5.1, and Bun 1.3.14. The host is the same M1 Mac mini with 8 GiB RAM.

Access after parsing.

| Workload | Main | R5 | Node | Bun | R5 vs main |
|---|---:|---:|---:|---:|---:|
| records 16k / repeat | 0.018543 | 0.000948 | 0.002732 | 0.004534 | -94.89% |
| records 16k / random | 0.040258 | 0.018925 | 0.006650 | 0.009302 | -52.99% |
| records 16k / fields | 0.084471 | 0.043061 | 0.005324 | 0.009273 | -49.02% |
| records 16k / sequential | 0.035095 | 0.022049 | 0.003892 | 0.005757 | -37.17% |
| records 1m / repeat | 0.018548 | 0.000946 | 0.002781 | 0.004566 | -94.90% |
| records 1m / random | 0.048808 | 0.023337 | 0.010568 | 0.010756 | -52.19% |
| records 1m / fields | 0.107893 | 0.041208 | 0.010797 | 0.014336 | -61.81% |
| records 1m / sequential | 0.040623 | 0.016191 | 0.008220 | 0.007489 | -60.14% |
| records 20m / repeat | 0.005640 | 0.000953 | 0.003056 | 0.004864 | -83.10% |
| records 20m / random | 0.026262 | 0.025463 | 0.008614 | 0.010049 | -3.04% |
| records 20m / fields | 0.027233 | 0.028066 | 0.011731 | 0.014584 | +3.06% **regression** |
| records 20m / sequential | 0.016030 | 0.015589 | 0.006881 | 0.008264 | -2.75% |

Parse, stringify, and consumption controls.

| Workload | Main | R5 | Node | Bun | R5 vs main |
|---|---:|---:|---:|---:|---:|
| records 16k / scan | 58.611 | 22.072 | 40.364 | 35.518 | -62.34% |
| records 1m / scan | 2920.215 | 1368.690 | 2726.825 | 2201.265 | -53.13% |
| records 8m / scan | 23841.219 | 11917.125 | 31540.500 | 21518.250 | -50.01% |
| records 20m / scan | 54775.625 | 54825.875 | 84169.688 | 53111.812 | +0.09% |
| records 20m / roundtrip | 105599.250 | 105543.125 | 100035.750 | 68824.000 | -0.05% |
| records 1m / parse | 914.215 | 914.755 | 2633.815 | 2151.500 | +0.06% |
| records 1m / stringify | 640.699 | 639.469 | 838.012 | 957.805 | -0.19% |
| small record / parse | 0.096569 | 0.096757 | 0.347807 | 0.243590 | +0.20% |
| small record / stringify | 0.042156 | 0.042440 | 0.107602 | 0.118644 | +0.67% **regression** |
| long string 1m / stringify | 33.430 | 33.539 | 98.879 | 92.680 | +0.33% |

The 16k/1m/8m scan rows still beat both peers, with all candidate sample ranges below both peer ranges. The 20m scan and roundtrip medians remain behind Bun. Small parse and long-string stringify differences overlap main; they are not established improvements or resolved regressions.

Short-loop controls time **one complete reduction call**, including its setup and caller bookkeeping. Each trial makes one million calls after 5,000 warmup calls; nine trials per engine. Input is the 16k fixture. They do not divide the total by inner trip count.

| Inner iterations per call | Main | R3 | R5 | Node | Bun |
|---|---:|---:|---:|---:|---:|
| 1 | 0.023679 | 0.009223 | 0.013826 | 0.004382 | 0.007292 |
| 8 | 0.159176 | 0.043770 | 0.014383 | 0.008081 | 0.010712 |
| 64 | 1.201 | 0.331161 | 0.043337 | 0.035908 | 0.102542 |

The one-time probe pays off against R3 at eight and 64 iterations, but hurts the one-iteration case. Node still wins all three short-loop CPU medians; Bun wins at one/eight iterations. R5 remains faster than main in all three. A profitable dispatch for short loops remains an open requirement.

Peak RSS medians are **MiB for the whole fresh process**, including input, runtime, and output storage. These measurements are not retained-heap measurements.

| Workload | Main | R5 | Node | Bun |
|---|---:|---:|---:|---:|
| records 16k / repeat | 13.05 | 13.03 | 57.75 | 34.81 |
| records 16k / random | 13.42 | 13.47 | 57.75 | 35.41 |
| records 16k / fields | 13.17 | 13.23 | 57.92 | 36.00 |
| records 16k / sequential | 13.17 | 13.05 | 57.77 | 35.23 |
| records 1m / repeat | 17.56 | 17.58 | 63.69 | 37.94 |
| records 1m / random | 18.47 | 18.52 | 66.52 | 38.92 |
| records 1m / fields | 18.28 | 18.33 | 66.72 | 40.31 |
| records 1m / sequential | 18.28 | 17.59 | 66.53 | 39.20 |
| records 20m / repeat | 96.91 | 97.00 | 194.73 | 93.92 |
| records 20m / random | 96.94 | 97.02 | 194.75 | 94.75 |
| records 20m / fields | 96.94 | 97.02 | 195.02 | 95.34 |
| records 20m / sequential | 96.94 | 97.02 | 194.84 | 94.61 |
| records 16k / scan | 216.14 | 63.08 | 61.95 | 77.34 |
| records 1m / scan | 413.72 | 67.22 | 130.19 | 99.56 |
| records 8m / scan | 530.25 | 108.91 | 313.70 | 134.91 |
| records 20m / scan | 691.44 | 691.53 | 529.33 | 295.31 |
| records 20m / roundtrip | 506.34 | 506.42 | 493.91 | 319.33 |
| records 1m / parse | 67.11 | 67.19 | 125.50 | 95.09 |
| records 1m / stringify | 63.02 | 63.11 | 130.09 | 103.70 |
| small record / parse | 81.36 | 81.44 | 59.69 | 79.88 |
| small record / stringify | 33.34 | 33.44 | 59.78 | 394.58 |
| long string 1m / stringify | 54.52 | 54.55 | 255.39 | 157.72 |
| records 16k / short 1 | 13.19 | 13.03 | 57.94 | 36.19 |
| records 16k / short 8 | 13.20 | 13.03 | 58.16 | 35.34 |
| records 16k / short 64 | 13.19 | 13.03 | 58.03 | 34.88 |

Large-input memory remains unresolved: 20m scan peaks at about 692 MiB versus Bun’s 295 MiB in this window. The 1m and 8m scans preserve the earlier large reductions from main, at about 67 and 109 MiB.

The [focus-worker object hashes](results/invariant-field-loop-r5-validation/r3-r5-object-hashes.json) show that R5’s `worker.o` is byte-for-byte identical to R3’s. Their linked binaries differ. This rules out changed code in that generated object as a cause of R5-versus-R3 focus variation; runtime/link changes and measurement effects remain possible. The tiny-stringify regression against main remains unresolved.

Validation used the frozen source and an identical three-package release build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. The build took 5m17s. All three artifacts were newer than build start and copied into a hash-verified frozen directory. Worker links use the same flags as the main reference. The existing `relevant_box_roots` warning also appears in the actual main, R3, and R4 build logs.

- 305 runtime JSON tests, 10 existing codegen JSON tests, and both new loop tests pass. Additional codegen checks pass: 13 indexing, 26 property access, 29 element-shape loop, and 15 GC-call-effect tests. Runtime tests use one test thread.
- The [new behavior fixture](../../test-files/test_json_invariant_field_loop.ts) and existing projection fixture match Node in all 18 auto/tape/direct × normal/scheduled/full-GC executions. Every scheduled execution proves live copying and protected retired sets. Nine mutation controls and all 12 access checksums also pass.
- All three short-loop binaries match Node at one/eight/64 iterations in normal and scheduled modes, with positive moving/protection witnesses in all nine scheduled runs.
- The two principal native workers have **zero static root hazards**: 18 functions, 582 statepoints, 1,239 relocates. The short worker separately has zero hazards: 13 functions, 148 statepoints, 316 relocates. Both whole-fixture shadow checks pass.
- The whole native check is **not clean**: four unsuppressed findings across 101 functions, 1,889 statepoints, and 7,378 relocates. All four fingerprints are independently reproduced by compiling these exact current fixtures with the actual frozen main compiler/libraries and the same checker. They remain unresolved; no exemption or suppression was added.
- Script lint passes 73/74 checks. Public benchmark evidence freshness fails; the compile tier was skipped. This is not a claim that all PR checks pass.

All **751 timed checksums**, CPU/RSS sample vectors, medians, source-patch hashes, and quiet-window records were independently verified. Windows are archived before subsequent remote activity:

| Window (UTC, 2026-09-10) | Time | Load before → after | Evidence |
|---|---|---:|---|
| access | 13:58:53–13:59:16 | 1.067 → 1.499 | [Raw results](results/quiet-invariant-field-loop-r5-access/) |
| focus | 13:59:19–14:01:58 | 1.459 → 2.337 | [Raw results](results/quiet-invariant-field-loop-r5-focus/) |
| short | 14:02:01–14:02:24 | 2.230 → 2.159 | [Raw results](results/quiet-invariant-field-loop-r5-short/) |

[Validation artifacts](results/invariant-field-loop-r5-validation/) include the build/source provenance, emitted IR, native code, live GC witnesses, unit logs, exact-main root comparisons, short-loop sources, and independent analysis.

The original 38 parse/stringify rows, the additional consumption/access controls, rotating inputs, peak RSS, and retained RAM all remain in scope. The full matrices and retained-memory suite were not rerun on this rejected candidate; their last broad measurements remain in [the frozen-main report](MERGED_MAIN_53DF.md).

The next work is to remove the mixed-field and tiny-stringify regressions, then improve short-call setup and changing-index access without losing the long-loop win. Raising the 16 MiB lazy threshold still requires full/mixed consumption and retained-memory evidence. Main must be updated and rebuilt before any landing attempt.

The later [R6 index-cache placement experiment](INDEX_CACHE_DELAY.md) preserves the repeat win and improves smaller changing-index controls, but remains rejected for large-input regressions.

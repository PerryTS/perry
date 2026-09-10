# Deferred index-cache load experiment (R6)

**Not accepted for landing.** Moving an unused Array-subclass cache load out of the ordinary array path improves all six 16k/1m changing-index controls by **1.03–2.57% against R5**, with separated sample ranges. It does not fix the 20m mixed-field regression: that control remains **3.27% slower than frozen main**. It also makes 20m random access **1.19% slower than R5** and 20m scan **0.71% slower than main**; both are separated regressions. Fewer emitted instructions did not establish a better large-input result.

Small-record stringify is **0.042102 µs/op**, versus main's **0.042119** and R5's **0.042441** in the same window. R6/main ranges overlap; R6/R5 ranges are separated. This resolves neither the mechanism behind the earlier tiny-stringify slowdown nor the remaining large-input regressions. All samples, including first-trial effects, are retained.

R6 source is `3f7e40984a844a132f6446a04bfc6fd4ca03a235`, on `codex/json-index-cache-delay-r6`, based on [R5](INVARIANT_FIELD_LOOP.md). The frozen main reference is `53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530); the R5 reference is its measured source `24a8ab6953ba811f5112e49ba1c79ae6e683118b`. Latest observed main remains `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). This report does not measure that newer main build. Update/rebuild main before any future landing attempt.

The [compiler change](../../crates/perry-codegen/src/expr/index_get/inline_dyn_typed_array.rs) moves the native index-cache pointer load and null-sentinel selection into the shape-carried Object branch. The semantic fallback still receives the same static cache slot address. Ordinary Arrays, lazy arrays, and elements-backed subclasses bypass the unused cache load. All receiver/index/descriptor/forwarding guards and property semantics remain. Runtime and GC sources are unchanged relative to R5.

[Emitted-code evidence](results/index-cache-delay-r6-validation/cache-load-placement.json) proves all 12 access-worker index-cache loads moved from `tav.get.slow` into `arrlike.ic.shape` in emitted IR. The inspected ARM64 mixed-field name lookup removes five instructions (`adrp`, `add`, `ldr`, `cmp`, `csel`) from the common array path. Both [invariant repeat loops](results/index-cache-delay-r6-validation/repeat-loop-machine.json) retain their three-instruction `fadd`/`subs`/branch body. Their CPU medians remain 0.946–0.954 ns per iteration. All candidate samples beat both peers at 1m/20m; the 16k sample ranges overlap a peer. This does not establish a win for short calls or changing indices.

CPU medians below are **µs per operation**. Access rows measure one loop iteration after parsing once; “fields” retains the three-field workload. Focus rows include the named parse/stringify/scan work. Each row has seven interleaved fresh-process trials per engine: main, R5, R6, Node 26.5.1, and Bun 1.3.14, on the M1 Mac mini with 8 GiB RAM.

Access after parsing.
| Workload | Main | R5 | R6 | Node | Bun | R6 vs main | R6 vs R5 |
|---|---:|---:|---:|---:|---:|---:|---:|
| records 16k / repeat | 0.018550 | 0.000952 | 0.000946 | 0.002739 | 0.004521 | -94.90% | -0.63% |
| records 16k / random | 0.040415 | 0.018907 | 0.018589 | 0.006647 | 0.009300 | -54.00% | -1.68% |
| records 16k / fields | 0.084487 | 0.043048 | 0.042600 | 0.005343 | 0.009255 | -49.58% | -1.04% |
| records 16k / sequential | 0.035068 | 0.022049 | 0.021805 | 0.003915 | 0.005758 | -37.82% | -1.11% |
| records 1m / repeat | 0.018538 | 0.000948 | 0.000946 | 0.002763 | 0.004552 | -94.90% | -0.21% |
| records 1m / random | 0.048825 | 0.023425 | 0.022824 | 0.010570 | 0.010784 | -53.25% | -2.57% |
| records 1m / fields | 0.107948 | 0.041210 | 0.040582 | 0.010787 | 0.014316 | -62.41% | -1.52% |
| records 1m / sequential | 0.040420 | 0.016190 | 0.016024 | 0.008144 | 0.007470 | -60.36% | -1.03% |
| records 20m / repeat | 0.005640 | 0.000951 | 0.000954 | 0.003084 | 0.004723 | -83.09% | +0.32% |
| records 20m / random | 0.026346 | 0.025459 | 0.025761 | 0.008668 | 0.010090 | -2.22% | +1.19% |
| records 20m / fields | 0.027210 | 0.028039 | 0.028101 | 0.011890 | 0.014710 | +3.27% **regression** | +0.22% |
| records 20m / sequential | 0.016023 | 0.015625 | 0.015658 | 0.006883 | 0.008293 | -2.28% | +0.21% |

Parse, stringify, and consumption controls.
| Workload | Main | R5 | R6 | Node | Bun | R6 vs main | R6 vs R5 |
|---|---:|---:|---:|---:|---:|---:|---:|
| records 16k / scan | 58.689500 | 22.151750 | 22.116250 | 41.098250 | 35.718500 | -62.32% | -0.16% |
| records 1m / scan | 2918.920000 | 1370.855000 | 1366.900000 | 2690.815000 | 2203.600000 | -53.17% | -0.29% |
| records 8m / scan | 23885.468750 | 11915.937500 | 11910.000000 | 31237.625000 | 21550.281250 | -50.14% | -0.05% |
| records 20m / scan | 54775.812500 | 54835.812500 | 55167.062500 | 85830.500000 | 52167.062500 | +0.71% **regression** | +0.60% |
| records 20m / roundtrip | 105645.125000 | 105539.875000 | 105733.125000 | 98057.875000 | 68863.000000 | +0.08% | +0.18% |
| records 1m / parse | 914.930000 | 915.205000 | 911.970000 | 2685.765000 | 2154.225000 | -0.32% | -0.35% |
| records 1m / stringify | 640.246094 | 639.851562 | 639.859375 | 838.593750 | 957.636719 | -0.06% | +0.00% |
| small record / parse | 0.096717 | 0.096700 | 0.096627 | 0.356579 | 0.244472 | -0.09% | -0.08% |
| small record / stringify | 0.042119 | 0.042441 | 0.042102 | 0.107661 | 0.118336 | -0.04% | -0.80% |
| long string 1m / stringify | 33.651367 | 33.266357 | 34.085205 | 99.154541 | 92.781982 | +1.29% | +2.46% |

The 16k/1m/8m scan controls still beat both peers across their complete sample ranges. All nine changing-index access medians still trail both peers. The 20m scan and roundtrip controls remain behind Bun. The long-string stringify median is higher than both Perry references, but its full ranges overlap; it is unresolved rather than an established separated regression.

Peak RSS medians are **MiB for the whole fresh process**, not retained heap. No retained-memory result is inferred from these values.

| Workload | Main | R5 | R6 | Node | Bun |
|---|---:|---:|---:|---:|---:|
| records 16k / repeat | 13.05 | 13.03 | 13.05 | 57.73 | 34.80 |
| records 16k / random | 13.42 | 13.47 | 13.48 | 57.78 | 35.41 |
| records 16k / fields | 13.17 | 13.23 | 13.25 | 57.94 | 36.00 |
| records 16k / sequential | 13.17 | 13.05 | 13.06 | 57.80 | 35.25 |
| records 1m / repeat | 17.58 | 17.56 | 17.59 | 63.55 | 37.92 |
| records 1m / random | 18.47 | 18.52 | 18.53 | 66.56 | 38.92 |
| records 1m / fields | 18.28 | 18.33 | 18.34 | 66.73 | 40.33 |
| records 1m / sequential | 18.28 | 17.59 | 17.59 | 66.52 | 39.20 |
| records 20m / repeat | 96.92 | 97.00 | 97.08 | 194.72 | 93.94 |
| records 20m / random | 96.94 | 97.00 | 97.08 | 194.78 | 94.75 |
| records 20m / fields | 96.94 | 97.00 | 97.08 | 194.98 | 95.34 |
| records 20m / sequential | 96.92 | 97.00 | 97.08 | 194.89 | 94.61 |
| records 16k / scan | 216.14 | 63.08 | 63.08 | 61.94 | 77.41 |
| records 1m / scan | 413.72 | 67.22 | 67.22 | 130.28 | 99.58 |
| records 8m / scan | 530.25 | 108.91 | 108.91 | 313.55 | 137.30 |
| records 20m / scan | 691.44 | 691.53 | 691.53 | 529.34 | 326.20 |
| records 20m / roundtrip | 506.34 | 506.72 | 506.73 | 493.95 | 319.55 |
| records 1m / parse | 67.11 | 67.19 | 67.19 | 125.52 | 95.02 |
| records 1m / stringify | 63.02 | 63.11 | 63.12 | 130.12 | 103.69 |
| small record / parse | 81.36 | 81.42 | 81.44 | 59.70 | 79.88 |
| small record / stringify | 33.34 | 33.41 | 33.39 | 59.80 | 394.58 |
| long string 1m / stringify | 54.48 | 54.55 | 54.55 | 255.38 | 157.67 |

Large-input memory remains unresolved. The 20m scan peaks at about 692 MiB, versus Bun's 326 MiB in this window; 1m/8m scan retain the previous reductions to about 67/109 MiB.

Validation and provenance:

- The exact three-package release build (`perry`, `perry-runtime-static`, `perry-stdlib-static`) completed in 5m34s. Compiler and both archives were newer than build start, copied unchanged, and hash-verified. [Build provenance](results/index-cache-delay-r6-validation/build-provenance.json) records source and artifact hashes.
- The initial freeze assertion expected raw archive equality with R5 and failed. The runtime build script embeds the current commit even when runtime sources are identical. Independent byte comparison proves that the runtime archive differs **only in 38 bytes of that 40-byte commit string**; replacing only that string makes the entire archive equal. No binary or stamp was modified for execution. The stdlib archive also differs and is not linked into timed workers; no equality claim is made for it. The original freeze script, corrected future recipe, and [comparison](results/index-cache-delay-r6-validation/build-stamp-comparison.json) are retained. The successful production build was not rerun.
- All **85 codegen unit tests** pass: 13 indexing, 26 property access, two invariant-loop, 29 element-shape-loop, and 15 GC-call-effect tests. Runtime source is unchanged; the earlier R5 runtime test results are not represented as newly executed R6 tests.
- Both JSON fixtures match Node in all 18 auto/tape/direct × normal/scheduled/full-GC runs. Every scheduled run demonstrates positive copying and protected retired sets: 143 protected sets for the invariant fixture and 776 for the projection fixture. All nine mutation and 12 access controls pass.
- The additional dispatch probe, existing Array-subclass regression, and existing typed-array parameter/view regression match Node in all six paired R5/R6 executions. They cover dense and shape-carried receivers, inherited/index/field getters, holes, coercions, fractional/negative/OOB indices, and typed-array views.
- The principal native workers have **zero root hazards** over 18 functions, 582 statepoints, and 1,239 relocates. Both whole-fixture shadow checks pass. The whole native check retains **four unsuppressed findings**, whose fingerprints match the archived actual-main proof on these identical fixtures. No exemption was added, and this is not a whole-native-clean claim.
- Script lint passes 73/74; public benchmark evidence freshness fails. The compile tier was skipped and two checks require CI context. The Rust file-size gate passes. Existing build warnings are retained; this is not a claim that all PR checks pass.

All **770 timed checksums**, CPU/RSS sample vectors and medians, source-patch hashes, and quiet-window records were independently verified. Both terminal windows were archived before subsequent remote activity.

| Window (UTC, 2026-09-10) | Time | Load before → after | Evidence |
|---|---|---:|---|
| access | 14:46:48–14:47:16 | 1.761 → 1.704 | [Raw results](results/quiet-index-cache-delay-r6-access/) |
| focus | 14:47:19–14:50:29 | 1.728 → 2.321 | [Raw results](results/quiet-index-cache-delay-r6-focus/) |

[Validation artifacts](results/index-cache-delay-r6-validation/) retain sources, build records, emitted IR and ARM64, GC witnesses, unit logs, root comparisons, and analysis. The archive also includes explicitly unexecuted preparation/next-investigation notes; these are not test or timing evidence.

The original 38 parse/stringify rows, consumption/access controls, short calls, changing inputs, peak RSS, and retained RAM all remain in scope. Full matrices, short-call timings, rotating inputs, and retained-memory tests were not rerun on this rejected candidate. Their existing results remain unchanged in the [frozen-main report](MERGED_MAIN_53DF.md) and [R5 report](INVARIANT_FIELD_LOOP.md).

The next candidates should reduce stringify setup and repeated per-read dispatch in changing-index loops. The full stringify entry currently reserves a 0x110-byte stack frame and saves many registers before reaching the small-record shortcut; outlining its existing general fallback is an unimplemented investigation, not a performance claim. Changing-index loops need guarded own-data/shape/bounds/number proofs and precise side exits. This cache-placement experiment should not be landed on the smaller-input gains alone.

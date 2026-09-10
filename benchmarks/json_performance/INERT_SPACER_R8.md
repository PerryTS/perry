# Bounded JSON stringify inert spacers (R8)

**Not qualified for landing.** Literal/computed zero-spacing calls gain about 8.9× in the initial run and 9.7× in the longer recheck, with no GC-policy change. The initial key-list slowdown is +0.79% with separated ranges; its longer recheck is +0.81% with overlapping ranges. Longer ordinary small-record stringify is +0.43% with separated ranges. That fails the requested no-regression bar, so R8 remains an archived experiment; it is not proposed for merge.

The initial +0.13% sequential-access signal has overlapping ranges in the longer recheck (+0.16% median), so it is not confirmed by that recheck. All initial and longer samples remain in the evidence. None of these observations establishes that the extra floating comparison or stack-frame size causes a slowdown: the key-list route does not execute that comparison.

Source `98c2a02cf16c56f397186d089aaa9aeced24e758` on `codex/json-inert-spacer-r8`, based on freshly built main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). R3/R5/R6/R7 production changes are excluded. Four engines ran interleaved on the M1 Mac mini (8 GiB), seven fresh-process trials per row, Node 26.5.1 and Bun 1.3.14.

CPU is microseconds per operation. Access rows count one loop iteration after parsing; focus and options rows count a complete operation. Options inputs are eagerly parsed before timing. Peak RSS is MiB for the whole process, including input, runtime, output, and allocator storage; it is not retained heap. Negative CPU deltas are faster. All samples are retained. “Separated” means the seven observed ranges do not overlap, not a confidence interval or proof of causality.

The candidate leads both peers on 14/34 CPU medians and 24/34 peak RSS medians. These are expanded controls, not the original 38-row matrix.

## Access after parsing

Window: 2026-09-10T16:28:08Z to 2026-09-10T16:28:33Z; one-minute load 1.627 to 1.630. Quiet gate passed, with no detected competing workloads at either boundary.

| Workload | Main | R8 | Node | Bun | R8 vs main | Observed ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / repeat | 0.018548 | 0.018565 | 0.002754 | 0.004541 | +0.09% | overlap |
| records_array_16k / random | 0.040458 | 0.040474 | 0.006674 | 0.009290 | +0.04% | overlap |
| records_array_16k / fields | 0.084473 | 0.084437 | 0.005358 | 0.009259 | -0.04% | overlap |
| records_array_16k / sequential | 0.035054 | 0.035099 | 0.003893 | 0.005735 | +0.13% | separated slowdown |
| records_array_1m / repeat | 0.018555 | 0.018555 | 0.002780 | 0.004571 | +0.00% | overlap |
| records_array_1m / random | 0.048974 | 0.048810 | 0.010625 | 0.010747 | -0.33% | overlap |
| records_array_1m / fields | 0.107880 | 0.107889 | 0.010794 | 0.014326 | +0.01% | overlap |
| records_array_1m / sequential | 0.040361 | 0.040302 | 0.008192 | 0.007468 | -0.15% | overlap |
| records_array_20m / repeat | 0.005646 | 0.005650 | 0.003029 | 0.004808 | +0.07% | overlap |
| records_array_20m / random | 0.025436 | 0.025494 | 0.008566 | 0.010065 | +0.23% | overlap |
| records_array_20m / fields | 0.027179 | 0.027158 | 0.011698 | 0.014569 | -0.08% | overlap |
| records_array_20m / sequential | 0.015651 | 0.015508 | 0.006783 | 0.008263 | -0.91% | overlap |

Peak RSS:

| Workload | Main | R8 | Node | Bun |
|---|---:|---:|---:|---:|
| records_array_16k / repeat | 13.03 | 13.02 | 57.84 | 34.81 |
| records_array_16k / random | 13.41 | 13.39 | 57.89 | 35.42 |
| records_array_16k / fields | 13.16 | 13.14 | 58.00 | 35.98 |
| records_array_16k / sequential | 13.16 | 13.14 | 57.81 | 35.23 |
| records_array_1m / repeat | 17.56 | 17.53 | 63.72 | 37.94 |
| records_array_1m / random | 18.47 | 18.45 | 66.61 | 38.89 |
| records_array_1m / fields | 18.27 | 18.25 | 66.77 | 40.31 |
| records_array_1m / sequential | 18.27 | 18.25 | 66.59 | 39.22 |
| records_array_20m / repeat | 96.89 | 96.88 | 194.80 | 93.94 |
| records_array_20m / random | 96.92 | 96.91 | 194.77 | 94.77 |
| records_array_20m / fields | 96.92 | 96.91 | 195.02 | 95.34 |
| records_array_20m / sequential | 96.91 | 96.91 | 194.86 | 94.64 |

[Raw samples](results/quiet-inert-spacer-r8-access/timing.jsonl), [output verification](results/quiet-inert-spacer-r8-access/verify.jsonl), [CPU/RSS vectors and medians](results/quiet-inert-spacer-r8-access/summary.json), [qualified window](results/quiet-inert-spacer-r8-access/window.json).

## Parse, stringify, and consumption

Window: 2026-09-10T16:28:34Z to 2026-09-10T16:32:11Z; one-minute load 1.630 to 2.218. Quiet gate passed, with no detected competing workloads at either boundary.

| Workload | Main | R8 | Node | Bun | R8 vs main | Observed ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / scan | 58.726250 | 58.738000 | 40.403250 | 35.562250 | +0.02% | overlap |
| records_array_1m / scan | 2914.440000 | 2910.600000 | 2801.490000 | 2204.945000 | -0.13% | overlap |
| records_array_8m / scan | 23884.843750 | 23908.125000 | 31698.906250 | 21548.468750 | +0.10% | overlap |
| records_array_20m / scan | 54605.312500 | 54577.000000 | 84964.812500 | 52265.000000 | -0.05% | overlap |
| records_array_20m / roundtrip | 105399.750000 | 105367.625000 | 98861.125000 | 68677.375000 | -0.03% | overlap |
| records_array_1m / parse | 909.760000 | 911.745000 | 2657.565000 | 2146.595000 | +0.22% | overlap |
| records_array_1m / stringify | 642.695312 | 639.699219 | 838.191406 | 962.781250 | -0.47% | overlap |
| small_record / parse | 0.096639 | 0.096513 | 0.333171 | 0.243351 | -0.13% | overlap |
| small_record / stringify | 0.042149 | 0.042156 | 0.107951 | 0.118494 | +0.02% | overlap |
| long_string_1m / stringify | 34.082764 | 33.217285 | 99.177979 | 92.924805 | -2.54% | overlap |
| null / stringify | 0.006569 | 0.006570 | 0.026315 | 0.032584 | +0.02% | overlap |
| string_a / stringify | 0.009703 | 0.009695 | 0.029198 | 0.033415 | -0.09% | overlap |
| empty_object / stringify | 0.017623 | 0.017610 | 0.031172 | 0.033710 | -0.07% | overlap |
| tiny_object / stringify | 0.030445 | 0.030470 | 0.036367 | 0.044612 | +0.08% | overlap |
| object_1k / stringify | 0.111752 | 0.111983 | 0.190318 | 0.207926 | +0.21% | overlap |

Peak RSS:

| Workload | Main | R8 | Node | Bun |
|---|---:|---:|---:|---:|
| records_array_16k / scan | 216.12 | 216.14 | 61.97 | 77.41 |
| records_array_1m / scan | 413.70 | 413.73 | 130.34 | 99.61 |
| records_array_8m / scan | 530.23 | 530.25 | 313.70 | 136.72 |
| records_array_20m / scan | 691.42 | 691.44 | 529.58 | 322.55 |
| records_array_20m / roundtrip | 506.62 | 506.64 | 493.97 | 319.30 |
| records_array_1m / parse | 67.09 | 67.11 | 125.72 | 95.02 |
| records_array_1m / stringify | 63.00 | 63.02 | 130.27 | 103.67 |
| small_record / parse | 81.34 | 81.38 | 59.64 | 79.89 |
| small_record / stringify | 33.33 | 33.34 | 59.83 | 394.59 |
| long_string_1m / stringify | 54.47 | 54.48 | 255.44 | 159.78 |
| null / stringify | 13.00 | 13.02 | 59.69 | 134.20 |
| string_a / stringify | 13.00 | 13.02 | 59.69 | 134.22 |
| empty_object / stringify | 13.09 | 13.12 | 59.69 | 134.20 |
| tiny_object / stringify | 32.31 | 32.34 | 59.72 | 134.22 |
| object_1k / stringify | 33.34 | 33.36 | 61.88 | 70.94 |

[Raw samples](results/quiet-inert-spacer-r8-focus/timing.jsonl), [output verification](results/quiet-inert-spacer-r8-focus/verify.jsonl), [CPU/RSS vectors and medians](results/quiet-inert-spacer-r8-focus/summary.json), [qualified window](results/quiet-inert-spacer-r8-focus/window.json).

## Stringify arguments

Window: 2026-09-10T16:32:13Z to 2026-09-10T16:32:45Z; one-minute load 2.218 to 2.112. Quiet gate passed, with no detected competing workloads at either boundary.

| Workload | Main | R8 | Node | Bun | R8 vs main | Observed ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.049044 | 0.049167 | 0.109190 | 0.121809 | +0.25% | overlap |
| small_record / dynamic-zero | 0.441241 | 0.049519 | 0.247989 | 0.397230 | -88.78% | separated gain |
| small_record / zero | 0.441302 | 0.049358 | 0.249105 | 0.396511 | -88.82% | separated gain |
| small_record / pretty | 0.514465 | 0.513810 | 0.310495 | 0.540450 | -0.13% | overlap |
| small_record / keys | 0.482385 | 0.486195 | 0.610425 | 0.501900 | +0.79% | separated slowdown |
| small_record / callback | 1.033240 | 1.032520 | 0.630540 | 0.566260 | -0.07% | overlap |
| records_array_16k / pretty | 46.166000 | 46.178000 | 34.618000 | 46.140000 | +0.03% | overlap |

Peak RSS:

| Workload | Main | R8 | Node | Bun |
|---|---:|---:|---:|---:|
| small_record / plain | 33.42 | 33.44 | 59.58 | 207.81 |
| small_record / dynamic-zero | 33.80 | 33.45 | 59.66 | 207.88 |
| small_record / zero | 33.81 | 33.42 | 59.62 | 207.86 |
| small_record / pretty | 33.59 | 33.58 | 59.59 | 77.72 |
| small_record / keys | 32.73 | 32.73 | 59.69 | 71.17 |
| small_record / callback | 27.66 | 27.66 | 59.66 | 51.25 |
| records_array_16k / pretty | 24.38 | 24.41 | 55.66 | 39.73 |

[Raw samples](results/quiet-inert-spacer-r8-options/timing.jsonl), [output verification](results/quiet-inert-spacer-r8-options/verify.jsonl), [CPU/RSS vectors and medians](results/quiet-inert-spacer-r8-options/summary.json), [qualified window](results/quiet-inert-spacer-r8-options/window.json).

## Longer recheck

Same frozen binaries and workers; eleven interleaved fresh-process repetitions, five million access iterations and two million options iterations. Options warmup remains 5,000; access has no warmup. Counts differ from the initial run, so absolute results from different windows should not be treated as a same-workload A/B. All 704 additional checksums, full options outputs, vectors, medians, recorded hashes, and both quiet windows pass the independent analyzer.

### access

Window 2026-09-10T16:33:54Z to 2026-09-10T16:35:16Z; load 1.696 to 2.005; quiet gate passed.

| Workload | Main CPU | R8 CPU | Node CPU | Bun CPU | R8 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / repeat | 0.018546 | 0.018563 | 0.000957 | 0.001215 | +0.09% | overlap |
| records_array_16k / random | 0.040195 | 0.040212 | 0.005006 | 0.006108 | +0.04% | overlap |
| records_array_16k / fields | 0.087027 | 0.086957 | 0.003038 | 0.004216 | -0.08% | overlap |
| records_array_16k / sequential | 0.037118 | 0.037178 | 0.002035 | 0.002263 | +0.16% | overlap |
| records_array_1m / repeat | 0.018530 | 0.018555 | 0.000961 | 0.001216 | +0.14% | overlap |
| records_array_1m / random | 0.047467 | 0.047485 | 0.006126 | 0.006070 | +0.04% | overlap |
| records_array_1m / fields | 0.108728 | 0.108750 | 0.004722 | 0.005711 | +0.02% | overlap |
| records_array_1m / sequential | 0.040746 | 0.040675 | 0.003105 | 0.003050 | -0.17% | overlap |
| records_array_20m / repeat | 0.005648 | 0.005638 | 0.001016 | 0.001257 | -0.18% | overlap |
| records_array_20m / random | 0.025159 | 0.025128 | 0.005995 | 0.005660 | -0.12% | overlap |
| records_array_20m / fields | 0.029496 | 0.029484 | 0.007000 | 0.006305 | -0.04% | overlap |
| records_array_20m / sequential | 0.016433 | 0.016414 | 0.003141 | 0.003280 | -0.12% | overlap |

| Workload | Main RSS | R8 RSS | Node RSS | Bun RSS |
|---|---:|---:|---:|---:|
| records_array_16k / repeat | 13.03 | 13.02 | 57.88 | 34.81 |
| records_array_16k / random | 13.41 | 13.39 | 57.86 | 35.41 |
| records_array_16k / fields | 13.16 | 13.14 | 58.00 | 35.98 |
| records_array_16k / sequential | 13.16 | 13.14 | 57.88 | 35.25 |
| records_array_1m / repeat | 17.56 | 17.55 | 63.75 | 37.92 |
| records_array_1m / random | 18.47 | 18.45 | 66.66 | 39.52 |
| records_array_1m / fields | 18.27 | 18.25 | 66.77 | 40.95 |
| records_array_1m / sequential | 18.25 | 18.25 | 66.58 | 39.91 |
| records_array_20m / repeat | 96.89 | 96.88 | 194.73 | 93.95 |
| records_array_20m / random | 96.91 | 96.89 | 194.84 | 94.75 |
| records_array_20m / fields | 96.91 | 96.89 | 195.05 | 95.34 |
| records_array_20m / sequential | 96.92 | 96.91 | 194.86 | 94.61 |

[All timed samples](results/quiet-inert-spacer-r8-recheck-access/timing.jsonl), [output verification](results/quiet-inert-spacer-r8-recheck-access/verify.jsonl), [sample vectors](results/quiet-inert-spacer-r8-recheck-access/summary.json), [quiet window](results/quiet-inert-spacer-r8-recheck-access/window.json).

### options

Window 2026-09-10T16:35:17Z to 2026-09-10T16:37:02Z; load 2.245 to 1.863; quiet gate passed.

| Workload | Main CPU | R8 CPU | Node CPU | Bun CPU | R8 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / keys | 0.455641 | 0.459334 | 0.598835 | 0.482546 | +0.81% | overlap |
| small_record / plain | 0.044662 | 0.044852 | 0.108557 | 0.119711 | +0.43% | separated slowdown |
| small_record / zero | 0.436829 | 0.044881 | 0.247973 | 0.394612 | -89.73% | separated gain |
| small_record / dynamic-zero | 0.437138 | 0.045206 | 0.247570 | 0.394195 | -89.66% | separated gain |

| Workload | Main RSS | R8 RSS | Node RSS | Bun RSS |
|---|---:|---:|---:|---:|
| small_record / keys | 33.77 | 33.77 | 59.67 | 378.50 |
| small_record / plain | 33.42 | 33.44 | 59.62 | 378.56 |
| small_record / zero | 33.80 | 33.45 | 59.62 | 378.62 |
| small_record / dynamic-zero | 33.80 | 33.44 | 59.66 | 378.62 |

[All timed samples](results/quiet-inert-spacer-r8-recheck-options/timing.jsonl), [output verification](results/quiet-inert-spacer-r8-recheck-options/verify.jsonl), [sample vectors](results/quiet-inert-spacer-r8-recheck-options/summary.json), [quiet window](results/quiet-inert-spacer-r8-recheck-options/window.json).

## Implementation and build evidence

The runtime admits primitive true and either sign of numeric zero to four existing bounded output attempts when the replacer is absent. This covers computed spacers as well as literals. Boxed Number/String values retain coercions, and the later lazy-source shortcut keeps its original admission predicate. No GC policy, object representation, callback order, or cache is added or changed.

Both revisions were built clean with `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, then frozen. Compiler and both static archives have recorded hashes and modification times after build start. All four generated worker objects are byte-identical across main/candidate. Each binary is linked against its corresponding fresh runtime archive. The public stringify entry retains its address, but its stack frame grows from 272 to 288 bytes. LLVM lowers the source zero-bit test to a floating zero comparison; this is not an integer-only machine check.

[Build and source provenance](results/inert-spacer-r8-validation/provenance.json), [worker object comparison](results/inert-spacer-r8-validation/worker-object-comparison.json), [machine observations](results/inert-spacer-r8-validation/machine-observations.json).

## Validation and limits

- 292 runtime JSON unit tests pass on the final source, single-threaded.
- Main and candidate each pass 19 Node output checks: new spacer and prior callback/reentry fixtures across auto/tape/direct parsing and normal/scheduled/full GC, plus a callback-only scheduled control. Every scheduled run asserts positive protected page sets and moved objects. The new spacer fixture records 1,190 protected sets and 44,380/44,455/44,380 moved objects; callback-only records 16 protected sets and 13,305 moved objects.
- Main and candidate each pass 14 options checks: plain, literal zero, computed zero, pretty, key-list and callback small-record cases, plus a 16 KiB pretty array, under normal and scheduled GC.
- Full native IR checking reports nine UNSUPPRESSED hazards across six modules (eight unrooted global values and one stale allocation value). All match actual main fingerprints; native IR is byte-identical after removing only the first ModuleID file-path comment. Shadow IR is byte-identical without normalization, and both shadow checks pass. Native ordinary workers and callback-only controls have zero findings. This is not an all-clean static result.
- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. Rust file cap passes (replacer.rs: 1,985 lines). Full CI is not claimed.

[Behavioral validation](results/inert-spacer-r8-validation/candidate-fixture-validation.json), [options validation](results/inert-spacer-r8-validation/candidate-options-validation.json), [root comparison and fingerprints](results/inert-spacer-r8-validation/root-comparison.json), [lint log](results/inert-spacer-r8-validation/script-lint.log.gz).

### Existing baseline correctness gaps

A dedicated 24-case lazy-array probe runs each case in a separate process. All twelve two-record cases match Node. For 180-record arrays, plain canonical input matches, while plain whitespace and duplicate-key input return noncanonical raw JSON; all six numeric-zero/true cases crash with SIGSEGV; the three pretty cases match Node. Candidate outcomes and complete stdout match main for all 24 cases, including the failures. This preserves baseline behavior and is not a conformance pass. Widening the lazy shortcut would spread its raw-output mismatch, so the initial wider draft was rejected before performance measurement.

A separate fractional-spacing probe records another baseline difference: main and local Bun emit compact output for positive fractions below one, while pinned Node emits newlines without indentation. The candidate exactly preserves main output. This case is kept separate from passing Node checks, with the original discovery and output retained; no specification conclusion is asserted.

[Lazy main probes](results/inert-spacer-r8-validation/lazy-main-probes.json), [candidate probes](results/inert-spacer-r8-validation/lazy-candidate-probes.json), [scope decision](results/inert-spacer-r8-validation/lazy-scope-decision.md), [fraction baseline](results/inert-spacer-r8-validation/fraction-baseline.json), [candidate fraction check](results/inert-spacer-r8-validation/candidate-fraction.json).

All 952 timed checksums, CPU/RSS sample vectors, medians, source-patch hashes, recorded local source/worker hashes, and quiet windows were independently checked by [analyze.py](results/inert-spacer-r8-validation/analyze.py). Full output hashes are checked for focus/options; access checks the complete workload checksum.

The full original 38-row matrix, broader consumption, rotating-source, short-call, and retained-memory suites have not been rerun for this experiment. They remain requirements for the overall performance objective. Earlier main results are in [merged-main measurements](https://github.com/PerryTS/perry/blob/4fc51192eddce6efeb11fb70c5c10ba3c25978cc/benchmarks/json_performance/MERGED_MAIN_53DF.md). The no-regressions objective remains open.

Next experiment: retain the original compact admission block and attempt the newly eligible spacers only after it fails, preserving original arguments on fallback. Check actual machine code before timing; no benefit is assumed. [Investigation notes](results/inert-spacer-r8-validation/next-dispatch-investigation.md).

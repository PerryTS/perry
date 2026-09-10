# JSON source string length reuse (R11)

**Rejected for landing.** Changing-input Unicode parsing uses 47.42% less CPU and ASCII-string parsing 12.26% less, but small-record parsing is 0.94% slower initially and 0.88% slower in the longer recheck. All seven initial pairs and all eleven recheck pairs are slower, with separated sample ranges. This fails the requested no-regression requirement.

Source `3c7c194bcad5eaa1da151a8e398da146bbbb34ce` on `codex/json-source-length-r11`, forked from actual main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). R8/R9/R10 production experiments are excluded. The exact fresh-main build is reused by verified hashes; main workers, behavioral fixtures and IR were recompiled at R11 paths. Remote main was confirmed at this revision before the experiment.

An unescaped token of at least 512 KiB can reuse the source header’s UTF-16 length when surrounding bytes are ASCII and at most one eighth of the token length. A bounded tail check rejects any lead byte that could make the legacy malformed-byte counter consume the closing quote. The ASCII prefix proves the token starts at a decoder boundary. Checked subtraction then gives the token length without rescanning it. Otherwise the existing counter is used.

The output still uses the original JSON construction allocator and copies its bytes. This adds no source view, cache, GC policy, root registry entry or retained intermediate. Existing UTF-16 counting is already vectorized for large strings; this experiment removes that pass rather than replacing a scalar counter.

## Measurement

Four engines on the M1 Mac mini (8 GiB): main, R11, Node 26.5.1 and Bun 1.3.14. Eight equal-size source strings are preloaded outside timing. Rotating them defeats the existing single-source cache; repeated-input and input-selection cases remain separate controls. This is a bounded changing-input corpus, not a newly allocated string on every call. Selection cost is reported without subtraction.

Initial measurements have seven interleaved fresh-process repetitions. The longer small-record recheck has eleven repetitions and exactly four times the original count: 2,312,136 calls, with the same 5,000-call warmup. There are 464 timed trials, 64 calibration trials and 112 full-output verification trials overall. Every verification compares all eight source members and the actual final timed value against Node; rotating verification ends at multiple corpus indices.

CPU is microseconds per operation. Negative deltas are faster. “Separated” means observed sample ranges do not overlap; it is not a confidence interval or proof of causality. Every sample and outlier is retained. Peak RSS is whole-process MiB, including eight live input strings, outputs, runtime and allocator storage. It is not retained heap and cannot be directly compared with the single-input worker’s RSS.

Unicode parsing falls from 142.557 to 74.957 µs, versus Bun 63.922 and Node 440.438. ASCII-string parsing falls from 99.834 to 87.596 µs, versus Bun 69.746 and Node 371.869. Both still trail Bun. Rotating escaped strings and the 1 MiB record array have overlapping ranges versus main. Repeated-input and selection controls also have overlapping ranges. The overall CPU/memory goal remains open.

## Initial comparison

Quiet window 2026-09-10T18:16:33Z–2026-09-10T18:18:15Z, one-minute load 1.307→1.917. The quiet gate passed, with no competing workload detected at either boundary. The terminal window was archived before the next remote operation.

| Fixture / mode | Iterations | Main CPU | R11 CPU | Node CPU | Bun CPU | R11 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| small_record / rotating | 578034 | 0.493878 | 0.498540 | 0.337952 | 0.254956 | +0.94% | separated slowdown | 7/7 |
| small_record / same | 1546391 | 0.099088 | 0.099016 | 0.360627 | 0.246594 | -0.07% | overlap | 2/7 |
| small_record / select | 2000000 | 0.009427 | 0.009427 | 0.002659 | 0.003629 | -0.01% | overlap | 4/7 |
| records_array_1m / rotating | 174 | 943.563218 | 943.022989 | 2735.672414 | 2147.787356 | -0.06% | overlap | 2/7 |
| records_array_1m / same | 175 | 938.560000 | 938.960000 | 2682.571429 | 2156.428571 | +0.04% | overlap | 4/7 |
| records_array_1m / select | 2000000 | 0.011240 | 0.010917 | 0.003095 | 0.003850 | -2.88% | overlap | 4/7 |
| long_string_1m / rotating | 1293 | 99.833720 | 87.595514 | 371.868523 | 69.745553 | -12.26% | separated gain | 0/7 |
| long_string_1m / same | 3134 | 0.359923 | 0.359285 | 367.888641 | 65.566369 | -0.18% | overlap | 1/7 |
| long_string_1m / select | 2000000 | 0.011607 | 0.011798 | 0.003086 | 0.003848 | +1.64% | overlap | 5/7 |
| escaped_1m / rotating | 159 | 1012.553459 | 1011.433962 | 1725.389937 | 2078.440252 | -0.11% | overlap | 3/7 |
| escaped_1m / same | 160 | 992.681250 | 993.925000 | 1725.131250 | 2075.187500 | +0.13% | overlap | 5/7 |
| escaped_1m / select | 2000000 | 0.011680 | 0.011319 | 0.003141 | 0.003900 | -3.09% | overlap | 0/7 |
| unicode_1m / rotating | 1272 | 142.556604 | 74.956761 | 440.437893 | 63.922170 | -47.42% | separated gain | 0/7 |
| unicode_1m / same | 2816 | 0.356889 | 0.355469 | 438.015980 | 59.420099 | -0.40% | overlap | 3/7 |
| unicode_1m / select | 2000000 | 0.011442 | 0.011296 | 0.003141 | 0.003893 | -1.28% | overlap | 2/7 |

| Fixture / mode | Main peak RSS | R11 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.969 | 59.922 | 80.188 |
| small_record / same | 75.969 | 75.969 | 59.656 | 79.875 |
| small_record / select | 12.812 | 12.812 | 57.844 | 35.266 |
| records_array_1m / rotating | 76.297 | 76.281 | 128.875 | 98.312 |
| records_array_1m / same | 76.297 | 76.281 | 128.859 | 100.031 |
| records_array_1m / select | 22.578 | 22.594 | 67.594 | 42.500 |
| long_string_1m / rotating | 90.234 | 90.188 | 177.625 | 148.500 |
| long_string_1m / same | 24.609 | 24.609 | 200.922 | 128.609 |
| long_string_1m / select | 24.328 | 24.328 | 69.031 | 43.766 |
| escaped_1m / rotating | 65.000 | 64.984 | 89.641 | 77.750 |
| escaped_1m / same | 65.000 | 64.984 | 89.641 | 77.750 |
| escaped_1m / select | 23.531 | 23.531 | 68.500 | 43.281 |
| unicode_1m / rotating | 61.156 | 61.141 | 155.828 | 151.781 |
| unicode_1m / same | 22.703 | 22.688 | 224.266 | 153.469 |
| unicode_1m / select | 22.438 | 22.422 | 68.500 | 44.656 |

[Timing samples](results/quiet-source-length-r11-rotating/timing.jsonl), [calibration samples](results/quiet-source-length-r11-rotating/calibration.jsonl), [full-output verification](results/quiet-source-length-r11-rotating/verify.jsonl), [host and source provenance](results/quiet-source-length-r11-rotating/host.json), [quiet window](results/quiet-source-length-r11-rotating/window.json).

## Longer recheck

Quiet window 2026-09-10T18:21:06Z–2026-09-10T18:21:47Z, one-minute load 1.961→1.876. The quiet gate passed, with no competing workload detected at either boundary. The terminal window was archived before the next remote operation.

| Fixture / mode | Iterations | Main CPU | R11 CPU | Node CPU | Bun CPU | R11 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| small_record / rotating | 2312136 | 0.487845 | 0.492147 | 0.352025 | 0.244980 | +0.88% | separated slowdown | 11/11 |

| Fixture / mode | Main peak RSS | R11 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.969 | 59.906 | 80.172 |

[Timing samples](results/quiet-source-length-r11-recheck-rotating/timing.jsonl), [calibration samples](results/quiet-source-length-r11-recheck-rotating/calibration.jsonl), [full-output verification](results/quiet-source-length-r11-recheck-rotating/verify.jsonl), [host and source provenance](results/quiet-source-length-r11-recheck-rotating/host.json), [quiet window](results/quiet-source-length-r11-recheck-rotating/window.json).

## Build, behavior and GC validation

- Exact clean-source build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 334.72 seconds. Compiler and both static archives were frozen with hashes; all mtimes are after build start. All four generated worker objects are byte-identical to main and linked with the corresponding frozen runtime archive.
- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 295 tests pass. New coverage includes complete Unicode, non-ASCII surrounding syntax, ambiguous truncated tails, 20,000 arbitrary-byte cases against the existing scalar interpretation, and large full-entry strings with tracked allocation and exact bytes/lengths.
- Main and candidate each pass 19 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. Scheduled runs assert positive protected page sets and movement. The new large-string fixture has 102 protected sets and 17,478/17,540/17,478 moved objects; callback-only pressure has 16 sets and 13,305 moved objects.
- Full native static checking retains seven UNSUPPRESSED findings: five unrooted globals, one unrooted string handle and one stale allocation value. All match actual main fingerprints. Twelve IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass, and native ordinary/callback controls have zero findings. Seven versus R10’s nine reflects the changed fixture corpus, not a GC improvement. No allowance was increased and no all-clean static result is claimed.
- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.

[Build provenance](results/source-length-r11-validation/build-provenance.json), [reference main](results/source-length-r11-validation/reference-main.json), [worker comparison](results/source-length-r11-validation/worker-object-comparison.json), [behavioral checks](results/source-length-r11-validation/candidate-fixture-validation.json), [options checks](results/source-length-r11-validation/candidate-options-validation.json), [root comparison](results/source-length-r11-validation/root-comparison.json), [lint log](results/source-length-r11-validation/script-lint.log.gz).

Disassembly confirms that `parse_string_value` tests the large-token threshold and calls the new source-length helper. The parser and original byte constructor both retain their 96-byte frames. This identifies added dispatch instructions in the common string path; it does not prove that those instructions alone cause the small-record slowdown. A follow-up should investigate placing the metadata attempt inside existing large-string construction dispatch. That follow-up is not implemented or measured in R11.

[Main disassembly commands](results/source-length-r11-validation/main-parser-machine.json), [candidate disassembly commands](results/source-length-r11-validation/candidate-parser-machine.json).

## Known baseline gaps and unrun work

All 24 isolated lazy-array probes preserve actual main outcomes and complete stdout. All twelve two-record cases match Node. At 180 records, six zero/true spacing cases crash with SIGSEGV, two plain whitespace/duplicate-key cases return noncanonical raw JSON, and the remaining four cases pass. Preserving these failures is not conformance. The positive fractional-spacing difference between main/Bun and Node is also preserved separately, not counted as a Node pass.

[Lazy baseline](results/source-length-r11-validation/lazy-main-probes.json), [candidate lazy probes](results/source-length-r11-validation/lazy-candidate-probes.json), [fraction baseline](results/source-length-r11-validation/fraction-baseline.json), [candidate fraction probe](results/source-length-r11-validation/candidate-fraction.json).

[Initial verifier](results/source-length-r11-validation/analyze-rotating.py) and [recheck verifier](results/source-length-r11-validation/analyze-rotating-recheck.py) independently check every timed/calibration checksum, complete-output hash, sample vector/median, source/worker/corpus hash, patch and quiet window. [Initial analysis](results/source-length-r11-validation/rotating-analysis.json) and [longer analysis](results/source-length-r11-validation/rotating-recheck-analysis.json) contain every CPU/RSS vector. The [validation manifest](results/source-length-r11-validation/manifest.json) records original and archived file hashes.

The original full 38-row parse/stringify matrix plus consumption rows, broader access/rotating/retained/short-call suites and stringify-options timing were not rerun after this candidate failed the first control group. Prepared drivers are not measurement evidence. Their requirements remain open. R11 is parked without a PR or release version bump.

[R10 report](https://github.com/PerryTS/perry/blob/ae4496ef64485bf57cc728db32763c69e157faa2/benchmarks/json_performance/INERT_SPACER_INLINE_R10.md) records the previous rejected experiment. The earlier accepted JSON work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).

# JSON construction context (R12)

**Rejected for landing.** The longer recheck shows repeated-input small-record parsing +1.09%, with separated sample ranges. Changing-input small-record parsing remains +0.78%, with overlapping ranges. All eleven pairs are slower in both cases. The large-string gains remain, and 1 MiB record-array parsing improves, but the no-regression requirement is not met.

Measured source `cde41403e5539daa64f0049e7767322948663c0e` on `codex/json-construction-context-r12`, based on actual main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). R11’s source-length admission remains; R8/R9/R10 production changes are excluded. The original fresh-main build is reused by verified hashes. Main workers, fixtures and IR were recompiled at R12 paths; remote main was confirmed at this revision before the experiment.

The parser passes itself as a compile-time string-construction context. The constructor selects its large-leaf path before counting units; only that path reads source metadata. Other callers specialize to their existing allocation batch with no source metadata. This removes R11’s extra size check and source-argument loading from the parser. There is no virtual dispatch, new cache, source view, arena field or GC policy change.

Source-length reuse still requires an unescaped token of at least 512 KiB, ASCII surrounding bytes no larger than one eighth of the token, and a bounded tail proof preventing a malformed sequence from consuming the closing quote in the legacy counter. Output bytes are still copied into the existing tracked allocation. Large-string counting was already vectorized; the saving comes from avoiding that pass.

## Measurement

Four engines on the quiet M1 Mac mini (8 GiB): main, R12, Node 26.5.1 and Bun 1.3.14. Eight equal-size sources are preloaded outside timing. Rotating them defeats the existing single-source cache; repeated-input and selection cases remain separate controls. This is a bounded changing-input corpus, not a freshly allocated source on every call. Selection cost is not subtracted.

The initial 15 cases have seven interleaved fresh-process repetitions. The longer two-case small-record recheck has eleven repetitions and exactly four times each initial iteration count: 2,321,080 changing-input calls and 6,217,616 repeated-input calls, with the same 5,000-call warmup. Overall: 508 timed trials, 68 calibration trials and 116 full-output verification trials. Verification checks every corpus member and the actual final loop value against Node, with multiple rotating end indices.

CPU is microseconds per operation; negative deltas are faster. “Separated” means observed sample ranges do not overlap, not a confidence interval or proof of cause. Every sample and outlier is retained. Peak RSS is whole-process MiB, including eight live inputs, outputs, runtime and allocator storage; it is not retained heap and is not directly comparable with the single-input worker.

Changing-input Unicode parsing improves 47.52% and ASCII-string parsing 12.18%. The 1 MiB record array improves 0.99% with changing input and 1.23% with repeated input. Unicode and ASCII still trail Bun. Initially, small records slow 0.79% with changing input and 1.15% with repeated input, both with separated ranges. The recheck retains the repeated-input regression; overlapping changing-input ranges do not erase its positive median and eleven slower pairs. The other nine initial controls have overlapping ranges.

## Initial comparison

Quiet window 2026-09-10T18:47:23Z–2026-09-10T18:49:06Z, one-minute load 1.462→2.244. The quiet gate passed, with no competing workload detected at either boundary. Each terminal window was archived before the next remote operation.

| Fixture / mode | Iterations | Main CPU | R12 CPU | Node CPU | Bun CPU | R12 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| small_record / rotating | 580270 | 0.493837 | 0.497725 | 0.343949 | 0.255393 | +0.79% | separated slowdown | 7/7 |
| small_record / same | 1554404 | 0.099003 | 0.100144 | 0.344266 | 0.245210 | +1.15% | separated slowdown | 7/7 |
| small_record / select | 2000000 | 0.009435 | 0.009435 | 0.002664 | 0.003622 | +0.00% | overlap | 3/7 |
| records_array_1m / rotating | 175 | 942.480000 | 933.165714 | 2650.971429 | 2151.788571 | -0.99% | separated gain | 0/7 |
| records_array_1m / same | 176 | 940.829545 | 929.238636 | 2687.102273 | 2156.187500 | -1.23% | separated gain | 0/7 |
| records_array_1m / select | 2000000 | 0.011371 | 0.011338 | 0.003085 | 0.003865 | -0.28% | overlap | 3/7 |
| long_string_1m / rotating | 1317 | 99.599089 | 87.469248 | 371.637813 | 70.605163 | -12.18% | separated gain | 0/7 |
| long_string_1m / same | 3133 | 0.361315 | 0.361315 | 366.731567 | 66.131503 | +0.00% | overlap | 4/7 |
| long_string_1m / select | 2000000 | 0.011547 | 0.012115 | 0.003096 | 0.003875 | +4.92% | overlap | 4/7 |
| escaped_1m / rotating | 159 | 1012.477987 | 1011.716981 | 1726.396226 | 2078.257862 | -0.08% | overlap | 3/7 |
| escaped_1m / same | 160 | 993.700000 | 993.618750 | 1724.112500 | 2074.518750 | -0.01% | overlap | 4/7 |
| escaped_1m / select | 2000000 | 0.011264 | 0.011429 | 0.003123 | 0.003875 | +1.46% | overlap | 5/7 |
| unicode_1m / rotating | 1387 | 141.731795 | 74.385004 | 439.758472 | 63.134823 | -47.52% | separated gain | 0/7 |
| unicode_1m / same | 2830 | 0.357244 | 0.357597 | 439.029682 | 58.915901 | +0.10% | overlap | 5/7 |
| unicode_1m / select | 2000000 | 0.011468 | 0.011355 | 0.003102 | 0.003873 | -0.99% | overlap | 4/7 |

| Fixture / mode | Main peak RSS | R12 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.922 | 60.000 | 80.172 |
| small_record / same | 76.797 | 76.734 | 59.641 | 79.812 |
| small_record / select | 12.828 | 12.781 | 57.875 | 35.250 |
| records_array_1m / rotating | 76.312 | 76.188 | 128.875 | 99.969 |
| records_array_1m / same | 76.312 | 76.188 | 128.781 | 100.328 |
| records_array_1m / select | 22.594 | 22.547 | 67.641 | 42.500 |
| long_string_1m / rotating | 90.234 | 90.141 | 178.688 | 154.469 |
| long_string_1m / same | 24.625 | 24.578 | 198.078 | 158.562 |
| long_string_1m / select | 24.344 | 24.281 | 68.969 | 43.750 |
| escaped_1m / rotating | 65.016 | 64.938 | 89.688 | 77.750 |
| escaped_1m / same | 65.016 | 64.938 | 89.672 | 77.750 |
| escaped_1m / select | 23.531 | 23.484 | 68.484 | 43.281 |
| unicode_1m / rotating | 61.562 | 61.438 | 159.500 | 147.188 |
| unicode_1m / same | 22.719 | 22.672 | 217.734 | 128.484 |
| unicode_1m / select | 22.453 | 22.391 | 68.625 | 44.609 |

[Timing](results/quiet-construction-context-r12-rotating/timing.jsonl), [calibration](results/quiet-construction-context-r12-rotating/calibration.jsonl), [complete-output verification](results/quiet-construction-context-r12-rotating/verify.jsonl), [host and source provenance](results/quiet-construction-context-r12-rotating/host.json), [quiet window](results/quiet-construction-context-r12-rotating/window.json).

## Longer recheck

Quiet window 2026-09-10T18:53:40Z–2026-09-10T18:55:15Z, one-minute load 1.666→1.802. The quiet gate passed, with no competing workload detected at either boundary. Each terminal window was archived before the next remote operation.

| Fixture / mode | Iterations | Main CPU | R12 CPU | Node CPU | Bun CPU | R12 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| small_record / rotating | 2321080 | 0.487835 | 0.491656 | 0.344466 | 0.245751 | +0.78% | overlap | 11/11 |
| small_record / same | 6217616 | 0.092014 | 0.093019 | 0.342256 | 0.242148 | +1.09% | separated slowdown | 11/11 |

| Fixture / mode | Main peak RSS | R12 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| small_record / rotating | 32.000 | 31.891 | 59.953 | 80.156 |
| small_record / same | 146.594 | 146.516 | 59.672 | 79.875 |

[Timing](results/quiet-construction-context-r12-recheck-rotating/timing.jsonl), [calibration](results/quiet-construction-context-r12-recheck-rotating/calibration.jsonl), [complete-output verification](results/quiet-construction-context-r12-recheck-rotating/verify.jsonl), [host and source provenance](results/quiet-construction-context-r12-recheck-rotating/host.json), [quiet window](results/quiet-construction-context-r12-recheck-rotating/window.json).

## Validation

- Exact clean-source build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 331.36 seconds. Compiler and both static archives were frozen with hashes and mtimes after build start. All four generated worker objects are byte-identical to main and linked with the corresponding frozen runtime.
- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 296 tests pass. R11’s Unicode/malformed-byte coverage remains, including 20,000 arbitrary-byte cases and tracked large allocations. The added test checks small and large Unicode output through a parser with no managed source and through a batch-only constructor.
- Main and candidate each pass 19 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. Every scheduled run asserts positive protected page sets and moved objects. The large-string fixture has 102 protected sets and 17,478/17,540/17,478 moved objects; callback-only has 16 sets and 13,305 moved objects.
- Full native static checking retains seven UNSUPPRESSED findings: five unrooted globals, one unrooted string handle and one stale allocation value. All match actual main fingerprints. Twelve IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass. Native ordinary/callback controls have zero findings. No allowance was increased and this is not an all-clean static result.
- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.

[Build provenance](results/construction-context-r12-validation/build-provenance.json), [reference main](results/construction-context-r12-validation/reference-main.json), [worker comparison](results/construction-context-r12-validation/worker-object-comparison.json), [behavioral checks](results/construction-context-r12-validation/candidate-fixture-validation.json), [options checks](results/construction-context-r12-validation/candidate-options-validation.json), [root comparison](results/construction-context-r12-validation/root-comparison.json), [lint log](results/construction-context-r12-validation/script-lint.log.gz).

## Machine findings and next investigation

The parser’s extra large-size shift/test is gone. Its string constructor call receives the parser directly; the size test now sits at the constructor entry, with a tail call to a separate large constructor. The parser, both normal constructor specializations and both large constructor specializations use 96-byte frames. This source change did not eliminate the measured small-record regressions.

The cached small-object path returns before string construction, so its slowdown cannot be explained solely by that per-string dispatch. Both main and R12 enter a 512-byte parse_slow frame (96-byte save area plus 416 bytes) and a 912-byte reuse-helper frame (96 plus 816). Source and disassembly show the cached template copied to the stack: a 583-byte memcpy plus scalar header fields, and a separate 64-byte copy for a planned array. The indirect-symbol tables confirm the memcpy stub, which plain disassembly misleadingly labels relative to writev.

A concrete follow-up is to examine borrowing the immutable template and its planned array values while the existing GC suppression scope covers construction, then release the borrow before scheduling/cleanup hooks. A smaller cache-miss entry could also avoid the construction frame on misses. Reentrancy and rooting must be checked before implementation. This is an investigation lead, not a proven cause of R12’s regression or an implemented R13 change.

[Main constructor disassembly](results/construction-context-r12-validation/main-parser-machine.json), [candidate constructor disassembly](results/construction-context-r12-validation/candidate-parser-machine.json), [cached-entry disassembly](results/construction-context-r12-validation/cached-entry-machine.json), [main indirect symbols](results/construction-context-r12-validation/main-indirect-symbols.txt), [candidate indirect symbols](results/construction-context-r12-validation/candidate-indirect-symbols.txt).

## Known gaps and remaining scope

All 24 isolated lazy-array probes preserve actual main outcomes and complete stdout. All twelve two-record cases match Node. At 180 records, six zero/true spacing cases crash with SIGSEGV, two plain whitespace/duplicate-key cases return noncanonical raw JSON, and four remaining cases pass. The main/Bun versus Node positive fractional-spacing difference is also preserved separately. These baseline failures are not conformance passes.

[Lazy baseline](results/construction-context-r12-validation/lazy-main-probes.json), [candidate lazy probes](results/construction-context-r12-validation/lazy-candidate-probes.json), [fraction baseline](results/construction-context-r12-validation/fraction-baseline.json), [candidate fraction check](results/construction-context-r12-validation/candidate-fraction.json).

[Initial verifier](results/construction-context-r12-validation/analyze-rotating.py) and [recheck verifier](results/construction-context-r12-validation/analyze-rotating-recheck.py) independently check every timed/calibration checksum, complete-output hash, CPU/RSS sample and median, source/worker/corpus hash, patch and window. [Initial vectors](results/construction-context-r12-validation/rotating-analysis.json), [recheck vectors](results/construction-context-r12-validation/rotating-recheck-analysis.json), [validation manifest](results/construction-context-r12-validation/manifest.json). Initial staging verified 102 remote hashes; recheck staging verified 104, including unchanged binaries and corpus.

The original 38 parse/stringify rows plus consumption, broader access/rotating, retained-memory, short-call and stringify-options timing were not rerun after this candidate failed its first controls. Prepared drivers are not timing evidence. The full objective and all those qualification requirements remain open. R12 is parked without a PR or release version bump.

[R11 report](https://github.com/PerryTS/perry/blob/c5d55834dc9e1369d3b2c2aacc874a7866d663bb/benchmarks/json_performance/SOURCE_LENGTH_R11.md) records the preceding rejected experiment. Earlier accepted work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).

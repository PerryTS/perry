# Borrowed JSON object templates (R13)

**Rejected for landing.** Repeated-input small-record parsing improves 10.47% in the longer recheck, with separated sample ranges. Changing-input parsing remains 0.93% slower in all eleven pairs, with overlapping ranges. Its initial 0.82% slowdown had separated ranges. The persistent changing-input cost remains unresolved, so this combined candidate does not meet the no-regression requirement.

Measured source `4195995098ca382659efb6af9471488029d8561e` on `codex/json-borrowed-template-r13`, compared with actual main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). It retains R11/R12's source-length and construction-context changes; R8/R9/R10 production changes are excluded. The original fresh-main compiler and archives were reused by verified hashes, with workers, fixtures and IR recompiled at R13 paths. Remote main was independently reconfirmed before timing.

The cache-hit path now borrows its immutable construction template inside the existing GC suppression scope. Fresh objects and mutable arrays are still allocated for every parse. The initial cache-match predicate is inlined; its construction helper is called only on a match. Cache admission and bounds, root registration, collection hooks, allocator policy and output ownership remain unchanged.

The existing pending-collection hook runs before the cache is borrowed. The borrow ends before suppression is lifted and before cleanup/scheduling hooks. Source inspection found no user callback or cache-replacement path during construction; the existing GC scanner remains responsible for rewriting every cached heap pointer. This does not remove later tracing work.

## Measurements

Main, R13, Node 26.5.1 and Bun 1.3.14 ran on the quiet M1 Mac mini with 8 GiB RAM. Eight equal-size sources are preloaded outside timing. Rotation defeats the existing single-source cache; repeated-input and input-selection cases are separate controls. This is a bounded changing-input corpus, not a fresh source allocation on every call. Selection cost is not subtracted.

The initial 15 cases use seven interleaved fresh-process repetitions. The two-case recheck uses eleven repetitions and four times the initial work: 2,325,580 changing-input calls and 6,818,180 repeated-input calls, both with 5,000 warmup calls. Total: 508 timed trials, 68 calibration trials and 116 complete-output verification trials. Verification checks all eight corpus members plus the actual final loop value against Node, including multiple rotating end indices.

CPU is microseconds per operation; negative deltas are faster. Separated means observed sample ranges do not overlap, not a confidence interval or proof of cause. Every sample and outlier is retained. Peak RSS is whole-process MiB, including eight live inputs, outputs, runtime and allocator storage; it is not retained heap or directly comparable with the single-input worker.

Initial changing-input Unicode parsing improves 47.40%, ASCII-string parsing 11.93%, and 1 MiB record-array parsing 1.09%; repeated-input record-array parsing improves 1.00%. Unicode and ASCII still trail Bun. Repeated-input small-record parsing improves 9.74% initially and 10.47% in the recheck. The changing-input slowdown persists. The other nine initial controls have overlapping ranges.

In the longer recheck, small-record peak RSS is 31.969 MiB versus main’s 31.984 MiB with changing inputs and 146.578 MiB for both with repeated input. This shows no material peak-RSS change in those controls; it is not a retained-memory qualification.

## Initial comparison

Quiet window 2026-09-10T19:23:14Z–2026-09-10T19:24:57Z; one-minute load 1.402→1.971. The quiet gate passed and no competing workload was detected at either boundary. Each terminal window was archived before the next remote operation.

| Fixture / mode | Iterations | Main CPU | R13 CPU | Node CPU | Bun CPU | R13 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| small_record / rotating | 581395 | 0.493706 | 0.497767 | 0.337012 | 0.254583 | +0.82% | separated slowdown | 7/7 |
| small_record / same | 1704545 | 0.098416 | 0.088834 | 0.342637 | 0.245760 | -9.74% | separated gain | 0/7 |
| small_record / select | 2000000 | 0.009431 | 0.009440 | 0.002662 | 0.003622 | +0.10% | overlap | 6/7 |
| records_array_1m / rotating | 175 | 942.851429 | 932.617143 | 2702.725714 | 2154.377143 | -1.09% | separated gain | 0/7 |
| records_array_1m / same | 174 | 939.408046 | 930.022989 | 2798.649425 | 2152.729885 | -1.00% | separated gain | 0/7 |
| records_array_1m / select | 2000000 | 0.010820 | 0.010879 | 0.003072 | 0.003807 | +0.55% | overlap | 3/7 |
| long_string_1m / rotating | 1295 | 99.850193 | 87.939768 | 372.893436 | 70.187645 | -11.93% | separated gain | 0/7 |
| long_string_1m / same | 3099 | 0.360116 | 0.358825 | 370.372701 | 66.159406 | -0.36% | overlap | 4/7 |
| long_string_1m / select | 2000000 | 0.011588 | 0.011700 | 0.003105 | 0.003902 | +0.97% | overlap | 4/7 |
| escaped_1m / rotating | 159 | 1012.503145 | 1012.490566 | 1726.396226 | 2077.861635 | -0.00% | overlap | 4/7 |
| escaped_1m / same | 160 | 994.337500 | 993.768750 | 1723.737500 | 2074.537500 | -0.06% | overlap | 4/7 |
| escaped_1m / select | 2000000 | 0.011544 | 0.011200 | 0.003136 | 0.003881 | -2.98% | overlap | 1/7 |
| unicode_1m / rotating | 1372 | 141.740525 | 74.555394 | 439.897959 | 63.428571 | -47.40% | separated gain | 0/7 |
| unicode_1m / same | 2775 | 0.357117 | 0.356396 | 436.913514 | 59.336937 | -0.20% | overlap | 2/7 |
| unicode_1m / select | 2000000 | 0.011311 | 0.011435 | 0.003125 | 0.003861 | +1.09% | overlap | 4/7 |

| Fixture / mode | Main peak RSS | R13 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.969 | 59.859 | 80.172 |
| small_record / same | 81.312 | 81.312 | 59.609 | 79.844 |
| small_record / select | 12.812 | 12.828 | 57.875 | 35.250 |
| records_array_1m / rotating | 76.297 | 76.250 | 128.875 | 99.953 |
| records_array_1m / same | 76.297 | 76.234 | 128.922 | 100.094 |
| records_array_1m / select | 22.594 | 22.594 | 67.625 | 42.484 |
| long_string_1m / rotating | 90.219 | 90.219 | 177.562 | 148.516 |
| long_string_1m / same | 24.594 | 24.625 | 197.984 | 145.500 |
| long_string_1m / select | 24.328 | 24.328 | 69.031 | 43.766 |
| escaped_1m / rotating | 65.000 | 65.000 | 89.656 | 77.734 |
| escaped_1m / same | 65.000 | 65.000 | 89.578 | 77.734 |
| escaped_1m / select | 23.531 | 23.531 | 68.469 | 43.266 |
| unicode_1m / rotating | 61.156 | 61.141 | 158.672 | 151.656 |
| unicode_1m / same | 22.703 | 22.734 | 200.234 | 147.250 |
| unicode_1m / select | 22.438 | 22.438 | 68.594 | 44.625 |

[Timing](results/quiet-borrowed-template-r13-rotating/timing.jsonl), [calibration](results/quiet-borrowed-template-r13-rotating/calibration.jsonl), [full-output verification](results/quiet-borrowed-template-r13-rotating/verify.jsonl), [host and sources](results/quiet-borrowed-template-r13-rotating/host.json), [quiet window](results/quiet-borrowed-template-r13-rotating/window.json).

## Longer recheck

Quiet window 2026-09-10T19:26:46Z–2026-09-10T19:28:26Z; one-minute load 1.456→1.699. The quiet gate passed and no competing workload was detected at either boundary. Each terminal window was archived before the next remote operation.

| Fixture / mode | Iterations | Main CPU | R13 CPU | Node CPU | Bun CPU | R13 vs main | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| small_record / rotating | 2325580 | 0.487281 | 0.491830 | 0.347328 | 0.245045 | +0.93% | overlap | 11/11 |
| small_record / same | 6818180 | 0.091760 | 0.082151 | 0.338136 | 0.242002 | -10.47% | separated gain | 0/11 |

| Fixture / mode | Main peak RSS | R13 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.969 | 59.953 | 80.172 |
| small_record / same | 146.578 | 146.578 | 59.703 | 79.891 |

[Timing](results/quiet-borrowed-template-r13-recheck-rotating/timing.jsonl), [calibration](results/quiet-borrowed-template-r13-recheck-rotating/calibration.jsonl), [full-output verification](results/quiet-borrowed-template-r13-recheck-rotating/verify.jsonl), [host and sources](results/quiet-borrowed-template-r13-recheck-rotating/host.json), [quiet window](results/quiet-borrowed-template-r13-recheck-rotating/window.json).

## Validation and machine code

- Clean-source production build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 334.50 seconds. Compiler and both archives were frozen, hashed, and checked for mtimes after build start. All four generated worker objects match main byte-for-byte and link the corresponding frozen runtime.
- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 297 pass. The new unit exercises cache replacement immediately after a hit, distinct object/array identities, and retention of prior output. Existing large-string Unicode/malformed-byte and fallback coverage remains.
- Main and candidate each pass 28 Node behavioral runs across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. All scheduled checks assert positive protected page sets and moved objects. The new cache-mutation/retention fixture has 2,158 protected sets and 99,893 moved objects under auto/direct parsing; tape has 1,999 and 95,991. Callback-only has 16 and 13,305.
- Full native static checking retains seven UNSUPPRESSED findings: five unrooted globals, one unrooted string handle and one stale allocation value. All match actual main fingerprints: 83 functions, 2,226 statepoints and 8,372 relocates. All fourteen IR files match after removing only the first native ModuleID path comment; shadow IR needs no normalization. Both shadow variants pass, and native ordinary/callback controls have zero findings. No allowance was increased; this is not an all-clean static result.
- Script lint: 73/74 pass; public benchmark evidence freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.

[Build](results/borrowed-template-r13-validation/build-provenance.json), [main provenance](results/borrowed-template-r13-validation/reference-main.json), [worker comparison](results/borrowed-template-r13-validation/worker-object-comparison.json), [behavior](results/borrowed-template-r13-validation/candidate-fixture-validation.json), [options](results/borrowed-template-r13-validation/candidate-options-validation.json), [root comparison](results/borrowed-template-r13-validation/root-comparison.json), [lint](results/borrowed-template-r13-validation/script-lint.log.gz).

Disassembly confirms removal of the 583-byte whole-template memcpy and the separate 64-byte planned-array vector copy. The helper loads planned values directly from the cache. Its frame shrinks from 912 to 224 bytes, including saved registers; parse_slow remains 512 bytes. The remaining 64-byte external call initializes the output-value array with memset_pattern16, as verified against the indirect-symbol table; it is not a template copy. The cache borrow count is decremented before suppression restoration.

[Borrow/reentrancy audit](results/borrowed-template-r13-validation/borrow-audit.json), [machine comparison](results/borrowed-template-r13-validation/cached-machine-comparison.json), [main symbols](results/borrowed-template-r13-validation/main-cached-entry-machine.json), [candidate symbols](results/borrowed-template-r13-validation/candidate-cached-entry-machine.json).

## Limits and next investigation

All 24 lazy-array probes preserve actual main outcomes and complete stdout. All twelve two-record cases match Node. At 180 records, six zero/true spacing cases crash with SIGSEGV, two plain whitespace/duplicate-key cases return noncanonical raw JSON, and four remaining cases pass. The main/Bun versus Node positive fractional-spacing difference is separately preserved. These baseline failures are not conformance passes.

[Lazy main](results/borrowed-template-r13-validation/lazy-main-probes.json), [lazy candidate](results/borrowed-template-r13-validation/lazy-candidate-probes.json), [fraction baseline](results/borrowed-template-r13-validation/fraction-baseline.json), [fraction candidate](results/borrowed-template-r13-validation/candidate-fraction.json).

[Initial verifier](results/borrowed-template-r13-validation/analyze-rotating.py) and [recheck verifier](results/borrowed-template-r13-validation/analyze-rotating-recheck.py) independently check timing/calibration checksums, full-output hashes, CPU/RSS vectors, source/worker/corpus hashes, patches and windows. [Initial vectors](results/borrowed-template-r13-validation/rotating-analysis.json), [recheck vectors](results/borrowed-template-r13-validation/rotating-recheck-analysis.json), [manifest](results/borrowed-template-r13-validation/manifest.json). Initial staging verified 102 remote hashes; recheck staging verified 104, including unchanged binaries and corpus.

The next experiment should isolate only the borrowed-template change on actual main, excluding R11/R12’s source-length and construction-context changes. This separates the established repeated-input benefit from the persistent changing-input cost; it does not assume which change caused that cost.

The original 38 parse/stringify rows plus consumption, broader access/rotating, retained-memory, short-call and stringify-options timing were not rerun after these controls failed qualification. Prepared drivers are not timing evidence. The full objective remains open. R13 is parked without a PR or release version bump.

[R12 report](https://github.com/PerryTS/perry/blob/72bfd454b960eb0005cbfaa90cbf76dcdcbd103c/benchmarks/json_performance/CONSTRUCTION_CONTEXT_R12.md) records the preceding rejected experiment. Earlier accepted work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).

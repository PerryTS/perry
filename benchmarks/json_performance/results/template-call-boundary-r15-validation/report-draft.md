# Borrowed JSON templates with the original call boundary (R15)

**Rejected for landing.** Small-record parsing improves 12.21% and object_1k parsing improves 12.95%, but large-workload regressions persist. A longer recheck confirms 1 MiB object parsing is 0.84% slower, with separated ranges and all eleven pairs slower. Keeping the original call boundary did not remove the regression pattern seen in R14.

Measured source `72be88b8d1ecdce1f972b646bed7d3a259ec823c` on `codex/json-template-call-boundary-r15`, based on actual main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). R15 replaces R14’s inlined predicate/private helper split with the original `try_reuse_parse_object_template` function, explicitly `#[inline(never)]`. It retains borrowed-template construction and the two mutation/retention tests. R11/R12’s string-length and construction-context changes are excluded; parser and string-constructor source match main.

The existing pending collection runs before borrowing the cached plan. Construction occurs under the existing GC suppression scope; the borrow ends before suppression restoration, cleanup and scheduling. Every mutable object and nested array is newly allocated. Cache admission and bounds, root registration, collection hooks and allocator policy are unchanged.

## Measurements

Main, R15, Node 26.5.1 and Bun 1.3.14 ran on the quiet M1 Mac mini with 8 GiB RAM. CPU is microseconds per operation; negative deltas are faster. Peak RSS is whole-process MiB, including input, output, runtime and allocator storage. All samples and outliers are retained. Separated ranges describe the observed samples; they are not confidence intervals or proof of cause.

The screen was selected before R15 timing: seven R14 large-workload concerns, three small-object parse targets and two stringify controls. It retains the original work counts and seven interleaved fresh-process repetitions. The recheck was selected after that screen: two separated slowdowns at the same work counts as R14’s flat identical-main A/A controls, with four times the original iterations, eleven repetitions and unchanged warmup. R15 does not rerun those A/A controls.

Both windows passed the pre-existing quiet gate, detected no competing workload at either boundary, and were fully archived before the next remote operation. Total: 424 timed trials and 70 complete-output verification records. All timed checksums, output hashes, CPU/RSS vectors and medians were independently verified. Peak RSS is effectively unchanged.

## Predeclared 12-case screen

Window 2026-09-10T20:37:46Z–2026-09-10T20:39:40Z; one-minute load 2.316→2.062.

| Fixture / operation | Iterations | Main CPU | R15 CPU | Node CPU | Bun CPU | Delta | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| tiny_object / parse | 2000000 | 0.036811 | 0.036755 | 0.081971 | 0.045212 | -0.152% | overlap | 2/7 |
| tiny_object / stringify | 2000000 | 0.033948 | 0.033939 | 0.037170 | 0.043155 | -0.027% | overlap | 2/7 |
| small_record / parse | 1581369 | 0.098647 | 0.086607 | 0.351117 | 0.244112 | -12.205% | separated gain | 0/7 |
| small_record / stringify | 2000000 | 0.045700 | 0.045890 | 0.107842 | 0.119515 | +0.415% | overlap | 5/7 |
| object_1k / parse | 1961848 | 0.082042 | 0.071418 | 0.533588 | 0.227686 | -12.950% | separated gain | 0/7 |
| records_array_1m / scan | 69 | 2934.782609 | 2951.333333 | 2941.347826 | 2226.318841 | +0.564% | separated slowdown | 7/7 |
| records_object_1m / parse | 81 | 2009.098765 | 2023.493827 | 2847.506173 | 2153.703704 | +0.716% | separated slowdown | 7/7 |
| records_object_8m / parse | 10 | 15956.800000 | 16105.500000 | 33439.500000 | 20952.500000 | +0.932% | separated slowdown | 7/7 |
| records_array_20m / parse | 4 | 39394.250000 | 39696.750000 | 93966.000000 | 52991.250000 | +0.768% | separated slowdown | 7/7 |
| records_array_20m / sparse | 4 | 39454.750000 | 39715.500000 | 95826.750000 | 53113.500000 | +0.661% | overlap | 7/7 |
| records_array_20m / scan | 4 | 40403.750000 | 40690.000000 | 89596.250000 | 53934.000000 | +0.708% | separated slowdown | 7/7 |
| records_object_20m / parse | 4 | 39490.500000 | 39639.250000 | 94896.250000 | 53017.250000 | +0.377% | overlap | 6/7 |

| Fixture / operation | Main peak RSS | R15 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| tiny_object / parse | 32.281 | 32.281 | 59.578 | 69.078 |
| tiny_object / stringify | 32.344 | 32.328 | 59.688 | 129.812 |
| small_record / parse | 79.547 | 79.547 | 59.672 | 79.891 |
| small_record / stringify | 33.344 | 33.344 | 59.719 | 378.703 |
| object_1k / parse | 64.953 | 64.938 | 61.750 | 71.266 |
| records_array_1m / scan | 158.500 | 158.500 | 97.531 | 83.219 |
| records_object_1m / parse | 66.469 | 66.438 | 92.969 | 79.172 |
| records_object_8m / parse | 187.328 | 187.328 | 244.438 | 131.203 |
| records_array_20m / parse | 255.438 | 255.422 | 359.906 | 220.625 |
| records_array_20m / sparse | 255.422 | 255.438 | 359.688 | 220.594 |
| records_array_20m / scan | 255.438 | 255.438 | 330.719 | 223.344 |
| records_object_20m / parse | 255.422 | 255.438 | 359.828 | 220.484 |

[Raw timings](results/quiet-template-call-boundary-r15-screen-focus/timing.jsonl), [full-output checks](results/quiet-template-call-boundary-r15-screen-focus/verify.jsonl), [host and worker hashes](results/quiet-template-call-boundary-r15-screen-focus/host.json), [window](results/quiet-template-call-boundary-r15-screen-focus/window.json).

## Longer two-case recheck

Window 2026-09-10T20:40:46Z–2026-09-10T20:42:06Z; one-minute load 1.292→2.385.

| Fixture / operation | Iterations | Main CPU | R15 CPU | Node CPU | Bun CPU | Delta | Ranges | Slower pairs |
|---|---:|---:|---:|---:|---:|---:|---|---:|
| records_object_1m / parse | 324 | 1867.987654 | 1883.614198 | 2724.731481 | 2138.419753 | +0.837% | separated slowdown | 11/11 |
| records_array_20m / scan | 16 | 49614.687500 | 49953.000000 | 85394.500000 | 53314.812500 | +0.682% | overlap | 10/11 |

| Fixture / operation | Main peak RSS | R15 peak RSS | Node peak RSS | Bun peak RSS |
|---|---:|---:|---:|---:|
| records_object_1m / parse | 71.156 | 71.156 | 125.641 | 107.781 |
| records_array_20m / scan | 704.391 | 704.375 | 529.438 | 308.812 |

[Raw timings](results/quiet-template-call-boundary-r15-recheck-focus/timing.jsonl), [full-output checks](results/quiet-template-call-boundary-r15-recheck-focus/verify.jsonl), [host and worker hashes](results/quiet-template-call-boundary-r15-recheck-focus/host.json), [window](results/quiet-template-call-boundary-r15-recheck-focus/window.json).

## Correctness and machine code

- `RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib json`: 292 pass. The unit checks distinct object/array identities, mutable cache replacement immediately after a hit and retained output. The TypeScript fixture mutates earlier outputs and validates retained results across allocation pressure.
- Exact production build: `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`, 337.27 seconds. Compiler and both static archives were frozen with verified hashes and all three mtimes after build start. Main’s original fresh production artifacts were reused by verified hashes; workers, fixtures and IR were recompiled at R15 paths. All four worker objects match main byte-for-byte and link the corresponding frozen runtime.
- Main and candidate each pass 28 Node behavioral checks across auto/tape/direct parsing and normal/scheduled/full GC, plus 14 stringify-options checks. Scheduled checks assert positive protected retired page sets and moved objects. The cache fixture has 2,158 protected sets and 99,893 moved objects under auto/direct; tape has 1,999 and 95,991. Callback-only has 16 and 13,305.
- Native static analysis retains seven UNSUPPRESSED findings identical to main: five unrooted globals, one unrooted string handle and one stale allocation value. All fourteen IR files match after removing only the first native ModuleID path comment; shadow IR requires no normalization. Both shadow variants pass; ordinary-worker and callback native checks have zero findings. This is not an all-clean static result.
- Script lint: 73/74 pass; public benchmark input freshness fails. Compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.

[Units](results/template-call-boundary-r15-validation/unit-source.json), [build](results/template-call-boundary-r15-validation/build-provenance.json), [main provenance](results/template-call-boundary-r15-validation/reference-main.json), [worker objects](results/template-call-boundary-r15-validation/worker-object-comparison.json), [behavior](results/template-call-boundary-r15-validation/candidate-fixture-validation.json), [options](results/template-call-boundary-r15-validation/candidate-options-validation.json), [static comparison](results/template-call-boundary-r15-validation/root-comparison.json), [lint](results/template-call-boundary-r15-validation/script-lint.log.gz).

The original out-of-line reuse function remains present and the private hit helper is absent. Its frame falls from 912 to 224 bytes and the whole-template memcpy disappears. Borrowed array iteration also removes planned-array copies; the remaining memset_pattern16 initializes output values. parse_slow remains 512 bytes, with DirectParser at sp+0xa0 and the same-address constructor and parse_value callees. These static findings establish reduced copying and preserved call/stack structure, but do not establish the cause of the large-case slowdown.

[Borrow audit](results/template-call-boundary-r15-validation/borrow-audit.json), [reuse comparison](results/template-call-boundary-r15-validation/cached-machine-comparison.json), [main symbols](results/template-call-boundary-r15-validation/main-cached-entry-machine.json), [candidate symbols](results/template-call-boundary-r15-validation/candidate-cached-entry-machine.json), [parse call sites](results/template-call-boundary-r15-validation/parse-slow-layout.json).

## Remaining scope

All 24 lazy-array probes preserve main’s exit codes and complete stdout: all twelve two-record cases pass Node; at 180 records, six zero/true-spacing cases crash with SIGSEGV and two plain whitespace/duplicate-key cases return noncanonical raw JSON. The other four pass. The separately recorded main/Bun versus Node fractional-spacing difference is unchanged. These existing failures are not conformance passes.

[Lazy main](results/template-call-boundary-r15-validation/lazy-main-probes.json), [lazy candidate](results/template-call-boundary-r15-validation/lazy-candidate-probes.json), [fraction baseline](results/template-call-boundary-r15-validation/fraction-baseline.json), [fraction candidate](results/template-call-boundary-r15-validation/candidate-fraction.json).

The full 50-case matrix, access, rotating-input, extended short-call, stringify-options timings and 36 retained-output cases were not run after the screen and recheck failed qualification. Prepared drivers are not execution evidence. The full objective remains open. R15 is parked without a PR or release version bump.

A separate next experiment can reduce template-capture work on changing small inputs: construct one local plan directly and borrow array plans during root barriers, preserving validation and publication order. Main’s existing capture disassembly shows redundant intermediate copies. This does not explain R15’s large-case regression, and the independent experiment should start from main.

[Screen selection](results/template-call-boundary-r15-validation/screen-cases.json), [recheck selection](results/template-call-boundary-r15-validation/recheck-cases.json), [screen verifier](results/template-call-boundary-r15-validation/analyze-screen.py), [recheck verifier](results/template-call-boundary-r15-validation/analyze-recheck.py), [screen vectors](results/template-call-boundary-r15-validation/screen-analysis.json), [recheck vectors](results/template-call-boundary-r15-validation/recheck-analysis.json), [validation manifest](results/template-call-boundary-r15-validation/manifest.json). Staging verified 102 remote hashes.

[R14 report and A/A controls](https://github.com/PerryTS/perry/blob/ecd8ba1964a74f0b1eea9a44d369026ad32179ac/benchmarks/json_performance/TEMPLATE_ONLY_R14.md). Earlier accepted work landed through [merge train #10037](https://github.com/PerryTS/perry/pull/10037).

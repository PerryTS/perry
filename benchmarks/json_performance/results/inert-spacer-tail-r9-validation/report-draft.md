# JSON stringify inert-spacer helper (R9)

**Rejected for landing:** ordinary small-record stringify slows by 2.82% with separated sample ranges. Literal and computed zero spacing are about 9.3× faster than main; key-list stringify improves 1.92%. Those gains do not satisfy the requested no-regression bar. The remaining matrix was not run after the first seven controls rejected this version.

Source `b62773c0064c047d68478dfac8d9a5761a90a4cf` on `codex/json-inert-spacer-tail-r9`, based on main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). R3/R5/R6/R7/R8 production changes are excluded. The exact fresh-main build from R8 is reused by verified hashes, while its workers, behavioral fixtures and static IR are recompiled at the R9 input paths. This is not a second rebuild of main or a claim to measure any newer main revision.

The candidate leaves the original Rust compact admission block intact, then calls a separate `#[inline(never)]` helper for newly inert spacers. The helper admits only primitive true and either sign of numeric zero, tries the four existing bounded writers, and retains the original arguments on fallback. Lazy-source admission, object representation, callback order, and GC policy are unchanged.

## Measurement

Four engines on the M1 Mac mini (8 GiB), seven interleaved fresh-process repetitions, Node 26.5.1 and Bun 1.3.14. Plain/literal-zero/computed-zero run two million iterations; pretty/key-list one million; callback 200,000; the 16 KiB pretty array 1,000. Warmup is 5,000 for small records and eight for the array. Inputs are eagerly parsed before timing.

Quiet window: 2026-09-10T16:59:28Z to 2026-09-10T17:00:43Z, one-minute load 1.854 to 1.942. The quiet gate passes, with no detected competing workload at either boundary. The terminal window was archived before any subsequent remote operation.

CPU is microseconds per complete stringify operation; negative deltas are faster. All samples and outliers are retained. “Separated” means observed sample ranges do not overlap; it is not a confidence interval or proof of causality.

| Workload | Main | R9 | Node | Bun | R9 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| small_record / plain | 0.044581 | 0.045836 | 0.108460 | 0.119672 | +2.82% | separated slowdown |
| small_record / dynamic-zero | 0.436918 | 0.047138 | 0.247423 | 0.394029 | -89.21% | separated gain |
| small_record / zero | 0.436245 | 0.046978 | 0.247725 | 0.393641 | -89.23% | separated gain |
| small_record / pretty | 0.482491 | 0.480784 | 0.303652 | 0.525310 | -0.35% | separated gain |
| small_record / keys | 0.459078 | 0.450273 | 0.599044 | 0.484669 | -1.92% | separated gain |
| small_record / callback | 0.945855 | 0.946515 | 0.607385 | 0.581495 | +0.07% | overlap |
| records_array_16k / pretty | 46.105000 | 45.352000 | 33.490000 | 45.924000 | -1.63% | separated gain |

Peak RSS is MiB for the whole process, including input, runtime, output, and allocator storage. It is not retained heap.

| Workload | Main | R9 | Node | Bun |
|---|---:|---:|---:|---:|
| small_record / plain | 33.41 | 33.38 | 59.62 | 378.56 |
| small_record / dynamic-zero | 33.78 | 33.41 | 59.72 | 378.64 |
| small_record / zero | 33.78 | 33.39 | 59.59 | 378.62 |
| small_record / pretty | 33.77 | 33.72 | 59.62 | 239.36 |
| small_record / keys | 33.75 | 33.70 | 59.73 | 207.77 |
| small_record / callback | 32.50 | 32.45 | 59.66 | 97.69 |
| records_array_16k / pretty | 35.06 | 35.06 | 57.11 | 49.72 |

[All 196 timed samples](results/quiet-inert-spacer-tail-r9-options/timing.jsonl), [complete-output verification](results/quiet-inert-spacer-tail-r9-options/verify.jsonl), [all CPU/RSS vectors and medians](results/quiet-inert-spacer-tail-r9-options/summary.json), [quiet window](results/quiet-inert-spacer-tail-r9-options/window.json).

## Machine-code finding

The original Rust block does not retain its original machine code. Adding a second production caller makes LLVM emit calls to `try_primitive`, `stringify_record_output::try_object`, and `stringify_flat::try_object` from the public entry; main inlines all three. The heap-string attempt remains inlined. The public frame grows from 272 to 304 bytes; the accepted helper path has a separate 32-byte frame. The helper uses integer comparisons for true and both zero bit patterns in this build.

This identifies a concrete change to investigate, without proving that it alone causes the measured slowdown. A bounded follow-up is to strengthen only those three writer annotations from `#[inline]` to `#[inline(always)]`, leaving the new helper out of line. Source search finds only the public entry and the new helper as their production call sites; other references are test-only. That follow-up is not implemented or measured in R9.

[Machine observations and exact disassembly commands](results/inert-spacer-tail-r9-validation/machine-observations.json), [main entry](results/inert-spacer-tail-r9-validation/main-stringify-full.s.gz), [candidate entry](results/inert-spacer-tail-r9-validation/candidate-stringify-full.s.gz), [candidate helper](results/inert-spacer-tail-r9-validation/candidate-inert-helper.s.gz), [follow-up notes](results/inert-spacer-tail-r9-validation/next-inline-investigation.md).

## Build and validation

- Candidate built from clean, frozen source with `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`. All three artifact hashes are recorded and modification times are after build start. The recorded 590.88 seconds includes waiting for the broader unit-test build to release Cargo’s build-directory lock.
- All four generated worker objects match main byte-for-byte. Each worker is linked with its corresponding verified static runtime archive. All 19 input fixtures are staged and their hashes verified alongside the binaries (60 remote hashes total).
- 292 runtime JSON tests pass single-threaded on final source. An earlier narrower `json::` filter also passed 158 tests; both logs and exact commands are retained separately.
- Main and candidate each pass 19 Node checks across auto/tape/direct parsing and normal/scheduled/full GC, including callback-only GC activity; each also passes 14 options checks. Every scheduled run asserts positive protected page sets and moved objects. The new spacer fixture records 1,190 protected sets and 44,380/44,455/44,380 moved objects. Callback-only records 16 protected sets and 13,305 moved objects.
- Full native IR checking retains nine UNSUPPRESSED findings (eight unrooted global values, one stale allocation value). All match actual main fingerprints. All twelve IR files match main after removing only the first native ModuleID file-path comment; shadow files need no normalization. Both shadow checks pass. Native ordinary workers and callback-only controls have zero findings. This is not an all-clean static result.
- Script lint: 73/74 pass, public benchmark evidence freshness fails; compile tier and two CI-only checks are skipped. The Rust file cap passes. Full CI is not claimed.

[Build provenance](results/inert-spacer-tail-r9-validation/build-provenance.json), [reference-main provenance](results/inert-spacer-tail-r9-validation/reference-main.json), [object comparison](results/inert-spacer-tail-r9-validation/worker-object-comparison.json), [candidate behavioral checks](results/inert-spacer-tail-r9-validation/candidate-fixture-validation.json), [options checks](results/inert-spacer-tail-r9-validation/candidate-options-validation.json), [root comparison](results/inert-spacer-tail-r9-validation/root-comparison.json), [lint log](results/inert-spacer-tail-r9-validation/script-lint.log.gz).

## Known baseline gaps remain visible

All 24 separately isolated lazy-array cases exactly preserve main outcomes and complete stdout. For 180 records, six zero/true spacing cases crash with SIGSEGV, and plain whitespace/duplicate-key inputs return noncanonical raw JSON. The remaining large cases and all twelve two-record cases match Node. Preserving these failures is not a conformance pass. Normalizing zero to undefined would widen the existing raw-source shortcut and change behavior, so fallback retains the original spacer.

The fractional-spacing probe also preserves the separately recorded main/Bun versus Node output difference for positive fractions below one. It is excluded from passing Node checks and retained as an explicit baseline limitation.

[Lazy baseline](results/inert-spacer-tail-r9-validation/lazy-main-probes.json), [candidate lazy outcomes](results/inert-spacer-tail-r9-validation/lazy-candidate-probes.json), [fraction baseline](results/inert-spacer-tail-r9-validation/fraction-baseline.json), [candidate fraction check](results/inert-spacer-tail-r9-validation/candidate-fraction.json).

All 196 timed checksums, full output hashes, CPU/RSS sample vectors, medians, recorded source/worker/input hashes, source patch and quiet window are independently verified by [analyze.py --options-only](results/inert-spacer-tail-r9-validation/analyze.py). The full original 38-row parse/stringify matrix, broader consumption/access, rotating, retained-memory, and short-call suites were not rerun on this rejected candidate. Their requirements remain open.

[R8 results and longer recheck](https://github.com/PerryTS/perry/blob/25b534011257910155c51e5e4362e53b6ddad219/benchmarks/json_performance/INERT_SPACER_R8.md) document the preceding experiment. R9 and R8 use different source paths and some different work counts, so their absolute medians are not an isolated same-window A/B.

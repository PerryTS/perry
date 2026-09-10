# Cached-subtree construction (R20)

**Parked; no runtime PR or release bump.** Skipping already-cached subtrees produces only small full-scan CPU gains: 0.34% at 1 MiB and 0.88% at 8 MiB. Peak RSS falls 2.81 MiB and 6.09 MiB respectively. The recurring 1 MiB object-parse control is 0.59% slower in the screen with separated samples, then 0.74% slower in the longer recheck with 10/11 slower pairs. The recheck ranges overlap: this is a persistent adverse trend, **not** a second separated result. The small gains do not justify advancing this candidate under the no-regression requirement.

Measured source `66ae4aaa39c8dee54773393d1c78c64181ae532a`, based directly on main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). Earlier accepted JSON work merged through [#10037](https://github.com/PerryTS/perry/pull/10037). [R19's independent rebuild](https://github.com/PerryTS/perry/blob/87f571d734eddca2bf997561c17adb072f5132c1/benchmarks/json_performance/MAIN_REBUILD_R19.md) produced byte-identical compiler/runtime/stdlib artifacts and rules out rebuild drift in that environment; it does not identify the cause of these control changes.

## Change and scope

Within the existing minority-cache full-materialization admission, a single exact-capacity array receives cached values directly. The validated tape supplies source offsets and subtree ends, while the ordinary DirectParser builds missing values. Cached aliases and mutations survive without constructing replacements that would immediately be overwritten. The existing rooted publication check and cache patch remain.

Parse routing, sparse-read dispatch, the 16 MiB limit, scan thresholds and GC policy are unchanged. Construction uses the existing suppression window and array builder, including aggregate layout and old-to-young edge tracking. This is an independent candidate on main, not a combination with rejected R18. The 120-record/16 KiB sequential scan still cannot enter this full-materialization producer under its existing threshold.

The source-level skip is demonstrated by a unit test that observes no interned key from the skipped subtree, in addition to value and identity checks. Linked-symbol evidence alone is not a sampled-entry or phase-time measurement. No R20 profile was collected.

## CPU: all 18 screen rows

CPU is user plus system time per operation, in microseconds; values are medians of seven fresh-process trials. Negative delta means less candidate CPU than main. Large cases use the predeclared doubled original iteration count; tiny/small counts and warmups are unchanged. “Separated” compares the complete sample ranges in this window.

| Fixture / operation | Main µs | Candidate µs | Node µs | Bun µs | CPU delta | Slower pairs | Samples |
|---|---:|---:|---:|---:|---:|---:|---|
| tiny_object / parse | 0.037 | 0.037 | 0.082 | 0.045 | -0.19% | 0/7 | Overlap |
| tiny_object / stringify | 0.034 | 0.034 | 0.037 | 0.043 | +0.01% | 4/7 | Overlap |
| small_record / parse | 0.098 | 0.099 | 0.346 | 0.244 | +0.11% | 5/7 | Overlap |
| small_record / stringify | 0.046 | 0.046 | 0.108 | 0.119 | +0.43% | 6/7 | Overlap |
| object_1k / parse | 0.082 | 0.082 | 0.533 | 0.228 | -0.16% | 2/7 | Overlap |
| records_array_1m / scan | 2932.739 | 2922.645 | 2826.594 | 2233.341 | -0.34% | 0/7 | Faster, separated |
| records_object_1m / parse | 1918.259 | 1929.593 | 2770.988 | 2144.136 | +0.59% | 7/7 | Slower, separated |
| records_object_8m / parse | 15897.600 | 15931.800 | 32445.100 | 20951.400 | +0.22% | 7/7 | Overlap |
| records_array_20m / parse | 40350.375 | 40401.875 | 90323.750 | 53854.375 | +0.13% | 5/7 | Overlap |
| records_array_20m / sparse | 40306.875 | 40411.125 | 88145.500 | 53289.500 | +0.26% | 7/7 | Overlap |
| records_array_20m / scan | 41282.625 | 41357.625 | 84765.250 | 54159.750 | +0.18% | 6/7 | Overlap |
| records_object_20m / parse | 40381.250 | 40384.125 | 89204.500 | 53192.625 | +0.01% | 3/7 | Overlap |
| records_array_16k / parse | 13.499 | 13.488 | 39.445 | 33.790 | -0.09% | 3/7 | Overlap |
| records_array_16k / sparse | 20.220 | 20.291 | 39.445 | 33.836 | +0.35% | 7/7 | Slower, separated |
| records_array_16k / scan | 68.609 | 68.556 | 40.992 | 34.800 | -0.08% | 1/7 | Overlap |
| records_array_1m / parse | 908.279 | 909.788 | 2614.000 | 2142.268 | +0.17% | 6/7 | Overlap |
| records_array_1m / sparse | 965.988 | 964.575 | 2653.674 | 2177.189 | -0.15% | 2/7 | Overlap |
| records_array_8m / scan | 23676.500 | 23468.312 | 31039.000 | 21729.625 | -0.88% | 0/7 | Faster, separated |

## Peak RSS: the same screen

MiB, median process peak RSS. This includes the whole worker workload and is not a measurement of live output bytes or allocator capacity.

| Fixture / operation | Main MiB | Candidate MiB | Node MiB | Bun MiB | Candidate delta |
|---|---:|---:|---:|---:|---:|
| tiny_object / parse | 32.281 | 32.328 | 59.578 | 69.078 | +0.047 |
| tiny_object / stringify | 32.328 | 32.375 | 59.656 | 129.812 | +0.047 |
| small_record / parse | 79.578 | 79.594 | 59.672 | 79.906 | +0.016 |
| small_record / stringify | 33.344 | 33.391 | 59.672 | 378.688 | +0.047 |
| object_1k / parse | 64.922 | 64.969 | 61.891 | 71.281 | +0.047 |
| records_array_1m / scan | 300.641 | 297.828 | 130.375 | 93.219 | -2.812 |
| records_object_1m / parse | 71.156 | 71.203 | 125.719 | 88.734 | +0.047 |
| records_object_8m / parse | 312.172 | 312.219 | 266.875 | 141.266 | +0.047 |
| records_array_20m / parse | 393.172 | 393.219 | 402.484 | 231.719 | +0.047 |
| records_array_20m / sparse | 393.172 | 393.219 | 402.375 | 253.953 | +0.047 |
| records_array_20m / scan | 393.188 | 393.234 | 428.969 | 237.844 | +0.047 |
| records_object_20m / parse | 393.172 | 393.219 | 402.438 | 255.500 | +0.047 |
| records_array_16k / parse | 63.438 | 63.469 | 65.891 | 73.375 | +0.031 |
| records_array_16k / sparse | 73.734 | 73.781 | 66.047 | 71.109 | +0.047 |
| records_array_16k / scan | 504.984 | 505.094 | 66.047 | 77.578 | +0.109 |
| records_array_1m / parse | 67.109 | 67.141 | 125.578 | 110.938 | +0.031 |
| records_array_1m / sparse | 67.812 | 67.875 | 125.703 | 127.141 | +0.062 |
| records_array_8m / scan | 309.422 | 303.328 | 248.609 | 175.828 | -6.094 |

[Screen analysis](results/cached-construction-r20-validation/screen-analysis.json), [declarations](results/cached-construction-r20-validation/screen-cases.json), [raw screen window](results/quiet-cached-construction-r20-screen-focus/window.json).

## Longer recurring-control check

The predeclared 1 MiB object-parse check uses 324 iterations, two warmups and eleven interleaved fresh-process repetitions per engine. Candidate CPU is **1880.358 µs**, main **1866.509 µs**, Node **2741.846 µs**, Bun **2138.481 µs**. Candidate versus main is **+0.7420%**, with peak RSS **+0.04688 MiB**.

Ten paired CPU deltas are positive (about +0.55% to +1.03%); one is -0.054%. Full ranges overlap. The separate 16 KiB sparse-access slowdown is a screen finding only; it received no longer recheck. Neither observation is relabelled as a clean control.

[Recheck analysis](results/cached-construction-r20-validation/recheck-analysis.json), [predeclared control](results/cached-construction-r20-validation/recheck-cases.json), [raw recheck window](results/quiet-cached-construction-r20-recheck-focus/window.json).

## Validation and provenance

- All **294 JSON Rust tests** pass on the clean committed source. Three initial focused tests also pass, covering skipped decoding, bitmap word boundaries and pointer layout, and decline behavior.
- Both frozen arms pass **37 Node behavior runs** and **14 stringify-option checks**. The expanded fixture includes mixed/duplicate/escaped/Unicode/nested records, cached aliases and mutations, and four 2,600-element arrays that force materialization and subsequent collections. Its scheduled auto/tape runs have 854 protected retired sets and 159,027 moved objects per arm; direct runs have 1,118/155,168. These are actual moving/protected runs.
- All sixteen IR files match main after removing only the first native ModuleID path comment; shadow IR needs no normalization. Native analysis retains eight equal, unsuppressed findings: five unrooted globals, two string-handle warnings and one stale allocation value. Coverage: 2,775 safepoints, 2,097 live bundles, 10,213 relocates and 10,123 pairs. Shadow checks and ordinary-worker/callback native subsets pass. This is not an all-clean native result.
- All four worker object files are byte-identical at the common R20 source paths. Compiler and both archives are frozen from the exact production package set, with mtimes after build start and verified hashes. The build started at 23:24:55 UTC on 2026-09-10 and took 331.95 seconds. Main uses R19's independently rebuilt, byte-identical reference.
- Script lint passes 73/74 checks, with the existing public-benchmark freshness failure; the file cap passes. Compile tier and two CI-only checks were skipped; full CI is not claimed.
- All 24 lazy probes retain main's full stdout and exit outcomes, including six large zero/true-spacing SIGSEGV cases and two noncanonical raw outputs. The inherited fractional-spacing reference has matching compiler/runtime/fixture hashes, and the candidate preserves its difference from Node. These gaps remain gaps.

The first baseline setup failed because an unused optional fixture copy stopped the filename update, leaving a stale R18 subject name. Its script and the first eighteen passing baseline runs are preserved. After correcting and preflighting the fixture paths, the full baseline suite passed. Formatting also overlapped the start of the preliminary focused compile; the initial hash mismatch is recorded. The authoritative full JSON suite ran later on the final clean commit.

[Validation summary](results/cached-construction-r20-validation/validation-summary.json), [units](results/cached-construction-r20-validation/unit-source.json), [build](results/cached-construction-r20-validation/build-provenance.json), [source freshness](results/cached-construction-r20-validation/source.json), [static comparison](results/cached-construction-r20-validation/root-comparison.json), [construction review](results/cached-construction-r20-validation/construction-safety-review.md), [setup failure](results/cached-construction-r20-validation/initial-baseline-setup/failure.json), [formatting record](results/cached-construction-r20-validation/format-source-equivalence.json), [fractional reference reuse](results/cached-construction-r20-validation/fraction-reference-reuse.json).

## Preserved windows and remaining work

Both windows passed the predeclared quiet gate, and each terminal window was archived as the first subsequent remote operation. The screen ran 23:35:17–23:39:14 UTC, load 1.949 → 2.413. The recheck ran 23:40:01–23:40:33 UTC, load 1.724 → 1.953.

Total: **548 timed trials and 95 full-output verification trials**. Checksums, full-output hashes, declared iteration counts, every CPU/RSS sample vector and median, source patches, tool versions and 103 staged input hashes were checked. No full-50, access, rotating, retained-output, short-call or options performance qualification ran. Prepared drivers are not execution evidence. Parsing and stringify remain in the full objective.

Skipping a small cached fraction is insufficient here. The next investigation should quantify repeated value-string allocations and audit whether sharing such immutable values inside one construction window can safely remove more work. Its effect on stringify and retained outputs must be measured, and the recurring object-parse control remains mandatory. Code-placement effects remain a hypothesis, not a proven explanation for that control.

[Validation artifact manifest](results/cached-construction-r20-validation/manifest.json).

[Complete evidence index](results/cached-construction-r20-artifacts.json).

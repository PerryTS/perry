# Materialized lazy-array reads: R23

**Unmerged and not qualified for landing.** Access loops improve by up to 49.5%, and 1 MB/8 MB full scans improve by about 4%. The wider matrix also exposes regressions against pinned main. Five representative regressions persist in an independent 11-repetition run and are already present in the earlier R22 sparse-cache version when linked with identical benchmark object code.

Previously accepted JSON changes are merged through PR #10037, the merge train for closed PR #10036. R21 and R22 are separate unmerged experiments. R23 source is `fc877118c1914d27fd271852e5c7271a965693bd` on `codex/json-materialized-read-r23`; no R23 PR has been opened. The controlled reference is independently built main `1a9c0de6cb790d2467b0ca22a660870025179b37` (0.5.1531). Remote main advanced to `603b074ace01464bc66fc07cc8d532f26ccf5a0f` (0.5.1532) during this investigation; that inspected delta contains release/CI plumbing and a workspace patch-version bump, with no runtime/compiler implementation change. This report is still explicitly a comparison to pinned main 1a9.

The original 38 repeated-input parse/stringify medians remain faster than Node and Bun. That was already true of the reference; it does not establish general JSON dominance. Changing-input, retained-result and stringify-option performance controls were not rerun for R23 after the wider matrix found regressions.

## Implementation and correctness

R23 retains R22’s allocation-free sparse-cache hit and adds a dense read after full materialization. It resolves the live ordinary-array edge, checks descriptor flags and length/capacity, and returns an existing non-hole slot. Cold construction, holes, sparse indices, descriptors and other exceptional reads retain `lazy_get_rooted`. There is no production GC core, policy, threshold, parse-boundary or cache-admission change.

The linked ARM64 array accessor contains a materialized-hit block that calls only the nonallocating ordinary-array resolver, plus the unchanged call-free sparse-hit block. Both return without opening handle scopes. The outer generic-array frame is still 112 bytes. The disassembly’s 18- and 14-instruction block counts exclude receiver classification and the epilogue and are not timing measurements.

- 294 serial release JSON runtime tests pass (1.66 seconds of test execution). Coverage includes real `defineProperty` getter dispatch, mutation/growth/hole precedence, sparse identity and bitmap boundaries, and a materialized read after a witnessed copied-minor array move.
- Both frozen arms pass 46 Node behavior comparisons and 14 stringify-option comparisons. The expanded cached-read fixture performs allocations between materialized passes. Scheduled auto/tape runs each record 2,608 protected retired sets and 103,251 moved objects; direct mode records 2,588 and 103,263. Every scheduled fixture asserts positive protection and movement.
- All four benchmark object files are byte-identical between main and R23. All 18 native/shadow IR files match (only the first native ModuleID path comment is normalized). Shadow checks and the ordinary-worker/callback native subsets pass. The full native check retains **16 unsuppressed main findings: 15 unrooted, 1 stale**; this is not a clean full-native safety verdict. Coverage is 3,723 safepoints, 2,857 live bundles, 18,272 relocates and 18,182 safepoint/root pairs.
- Final local lint passes 73 of 74 executed checks, including file size and GC custody checks. The pre-existing public-benchmark freshness check fails; the compile tier and two CI-only checks were skipped. This is not a full CI pass.

Existing semantic failures remain visible: the 24-case lazy-spacer matrix has the same six SIGSEGV outcomes and two noncanonical outputs on both arms; the exact lazy-getter baseline still fails in auto/tape and passes in direct mode; fractional spacing still has the recorded main/Bun-versus-Node difference. These outcomes are preserved, not counted as conformance passes.

## Measurement scope

The quiet M1/8 GiB host used Node 26.5.1 and Bun 1.3.14. Each timing is a fresh process with interleaved engine order. All four windows passed the quiet gate and were archived before any subsequent remote operation. Analyzers verify checksums/output hashes, complete CPU/RSS sample vectors, medians, fixture/build/input hashes and source patches.

| Window | UTC, 2026-09-11 | Timed trials | Verification records |
|---|---|---:|---:|
| R23 access, 12 cases × 7 reps × 4 engines | 05:45:51–05:46:15 | 336 | 12 |
| R23 original 38 + 12 consumption, 7 reps | 05:47:22–05:54:12 | 1,400 | 250 |
| R23 five-regression recheck, 11 reps | 05:59:32–06:00:24 | 220 | 25 |
| R22 same-object-code control, 11 reps | 06:05:52–06:06:45 | 220 | 25 |

There are 1,956 R23 timed trials plus 220 R22 control trials. Verification records are separate from timed trials; the ordinary harness records two Node verifications per case and one per other engine. “Separated regression/gain” means the complete seven- or eleven-sample ranges do not overlap. Overlap does not establish equivalence. CPU is user + system time per iteration; RSS is whole-process peak RSS and does not measure live-heap size or prove a leak.

## Reads after one parse

Parsing is outside the timed interval. These are access-loop iterations, not JSON.parse latency. The fields loop performs three indexed reads (`id`, `name.length`, `active`) per iteration. These parse-once RSS numbers must not be substituted for the repeated parse/consumption RSS below.

Access CPU

| Fixture / operation | Main µs | R23 µs | Node µs | Bun µs | R23 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / repeat | 0.018561 | 0.009411 | 0.002729 | 0.004541 | -49.30% | overlap |
| records_array_16k / random | 0.040412 | 0.023628 | 0.006655 | 0.009309 | -41.53% | gain |
| records_array_16k / fields | 0.084419 | 0.055563 | 0.005334 | 0.009277 | -34.18% | gain |
| records_array_16k / sequential | 0.035116 | 0.026363 | 0.003894 | 0.005760 | -24.93% | gain |
| records_array_1m / repeat | 0.018621 | 0.009400 | 0.002791 | 0.004612 | -49.52% | gain |
| records_array_1m / random | 0.048890 | 0.032864 | 0.010574 | 0.010765 | -32.78% | gain |
| records_array_1m / fields | 0.107937 | 0.060631 | 0.010833 | 0.014352 | -43.83% | gain |
| records_array_1m / sequential | 0.040437 | 0.026051 | 0.008277 | 0.007509 | -35.58% | gain |
| records_array_20m / repeat | 0.005641 | 0.005646 | 0.003039 | 0.004859 | +0.09% | overlap |
| records_array_20m / random | 0.025965 | 0.025666 | 0.008631 | 0.010041 | -1.15% | gain |
| records_array_20m / fields | 0.027200 | 0.027193 | 0.011719 | 0.014616 | -0.03% | overlap |
| records_array_20m / sequential | 0.015630 | 0.015576 | 0.006806 | 0.008264 | -0.35% | overlap |

Access RSS

| Fixture / operation | Main MiB | R23 MiB | Node MiB | Bun MiB | R23 − main MiB |
|---|---:|---:|---:|---:|---:|
| records_array_16k / repeat | 13.031 | 13.031 | 57.875 | 34.812 | +0.000 |
| records_array_16k / random | 13.406 | 13.406 | 57.906 | 35.438 | +0.000 |
| records_array_16k / fields | 13.156 | 13.156 | 58.031 | 35.984 | +0.000 |
| records_array_16k / sequential | 13.156 | 13.156 | 57.859 | 35.234 | +0.000 |
| records_array_1m / repeat | 17.547 | 17.547 | 63.688 | 37.938 | +0.000 |
| records_array_1m / random | 18.469 | 18.453 | 66.656 | 38.891 | -0.016 |
| records_array_1m / fields | 18.266 | 18.250 | 66.766 | 40.312 | -0.016 |
| records_array_1m / sequential | 18.266 | 18.250 | 66.641 | 39.188 | -0.016 |
| records_array_20m / repeat | 96.891 | 96.906 | 194.766 | 93.938 | +0.016 |
| records_array_20m / random | 96.922 | 96.922 | 194.812 | 94.750 | +0.000 |
| records_array_20m / fields | 96.922 | 96.922 | 195.031 | 95.297 | +0.000 |
| records_array_20m / sequential | 96.922 | 96.906 | 194.891 | 94.609 | -0.016 |

No access row has a separated regression. RSS differs from main by at most 0.015625 MiB. Despite the gains, the fields loop remains 10.42× Node at 16 KB and 5.60× Node at 1 MB; this is still a material remaining gap.

## Original 38 parse/stringify rows and 12 consumption rows

The original iteration counts and warmups are retained in `full-cases.json`, with their reference commit/path/hash. “Sparse”, “scan” and “roundtrip” include parsing/consumption and differ from the parse-once access rows above.

Full matrix CPU

| Fixture / operation | Main µs | R23 µs | Node µs | Bun µs | R23 vs main | Ranges |
|---|---:|---:|---:|---:|---:|---|
| null / parse | 0.010627 | 0.010630 | 0.027349 | 0.019051 | +0.02% | overlap |
| null / stringify | 0.006566 | 0.006567 | 0.027213 | 0.031209 | +0.02% | overlap |
| string_a / parse | 0.012826 | 0.012826 | 0.031731 | 0.022548 | +0.00% | overlap |
| string_a / stringify | 0.009687 | 0.009697 | 0.030048 | 0.032065 | +0.11% | overlap |
| empty_object / parse | 0.023435 | 0.023432 | 0.045201 | 0.024186 | -0.01% | overlap |
| empty_object / stringify | 0.017607 | 0.017604 | 0.031976 | 0.032584 | -0.02% | overlap |
| tiny_object / parse | 0.036772 | 0.037310 | 0.081688 | 0.045188 | +1.46% | regression |
| tiny_object / stringify | 0.033987 | 0.033973 | 0.037156 | 0.043170 | -0.04% | overlap |
| small_record / parse | 0.098498 | 0.098594 | 0.349528 | 0.245481 | +0.10% | overlap |
| small_record / stringify | 0.045725 | 0.045750 | 0.108008 | 0.119282 | +0.06% | overlap |
| object_1k / parse | 0.081995 | 0.082002 | 0.531298 | 0.227690 | +0.01% | overlap |
| object_1k / stringify | 0.115075 | 0.115029 | 0.193791 | 0.211243 | -0.04% | overlap |
| records_array_16k / parse | 13.588355 | 13.603687 | 41.289702 | 33.828607 | +0.11% | overlap |
| records_array_16k / stringify | 10.971504 | 10.983474 | 13.176384 | 23.226736 | +0.11% | overlap |
| records_array_16k / sparse | 19.989197 | 20.038892 | 40.697382 | 33.963650 | +0.25% | regression |
| records_array_16k / scan | 58.831694 | 59.036958 | 39.304972 | 35.707790 | +0.35% | overlap |
| records_array_16k / roundtrip | 20.994647 | 21.506202 | 53.744092 | 57.699569 | +2.44% | regression |
| records_array_1m / parse | 918.741176 | 918.758824 | 2785.311765 | 2142.488235 | +0.00% | overlap |
| records_array_1m / stringify | 646.362445 | 645.257642 | 844.550218 | 966.336245 | -0.17% | overlap |
| records_array_1m / sparse | 968.801242 | 966.875776 | 2730.956522 | 2178.372671 | -0.20% | overlap |
| records_array_1m / scan | 2932.086957 | 2822.492754 | 2844.376812 | 2227.420290 | -3.74% | gain |
| records_array_1m / roundtrip | 1413.782609 | 1438.156522 | 3613.182609 | 3107.739130 | +1.72% | regression |
| records_object_1m / parse | 2009.481481 | 2022.493827 | 2823.790123 | 2152.703704 | +0.65% | regression |
| records_object_1m / stringify | 649.462882 | 649.030568 | 850.065502 | 966.279476 | -0.07% | overlap |
| records_array_8m / parse | 8272.937500 | 8283.500000 | 33373.812500 | 20892.875000 | +0.13% | overlap |
| records_array_8m / stringify | 4727.551724 | 4724.551724 | 7130.689655 | 8184.517241 | -0.06% | overlap |
| records_array_8m / sparse | 8869.400000 | 8849.866667 | 32738.933333 | 21318.600000 | -0.22% | overlap |
| records_array_8m / scan | 22682.500000 | 21811.750000 | 30198.250000 | 21433.125000 | -3.84% | gain |
| records_array_8m / roundtrip | 13134.636364 | 13249.545455 | 36986.454545 | 27824.454545 | +0.87% | overlap |
| records_object_8m / parse | 15940.600000 | 16022.200000 | 34047.800000 | 20814.200000 | +0.51% | regression |
| records_object_8m / stringify | 4768.448276 | 4770.620690 | 7169.586207 | 8137.724138 | +0.05% | overlap |
| records_array_20m / parse | 39736.500000 | 39935.000000 | 97270.500000 | 53172.500000 | +0.50% | regression |
| records_array_20m / stringify | 11799.833333 | 11779.833333 | 17341.750000 | 20190.166667 | -0.17% | overlap |
| records_array_20m / sparse | 39714.000000 | 39951.750000 | 95886.750000 | 53210.750000 | +0.60% | regression |
| records_array_20m / scan | 40807.500000 | 41058.500000 | 91473.750000 | 53517.000000 | +0.62% | regression |
| records_array_20m / roundtrip | 99300.500000 | 99461.500000 | 102100.500000 | 76895.500000 | +0.16% | overlap |
| records_object_20m / parse | 39653.250000 | 39925.500000 | 96501.000000 | 53176.250000 | +0.69% | regression |
| records_object_20m / stringify | 11744.083333 | 11761.583333 | 17307.416667 | 20221.000000 | +0.15% | overlap |
| numbers_1m / parse | 1580.421569 | 1550.235294 | 3125.333333 | 3204.225490 | -1.91% | gain |
| numbers_1m / stringify | 1012.238095 | 1011.619048 | 1972.741497 | 2936.244898 | -0.06% | overlap |
| long_string_1m / parse | 0.349608 | 0.348895 | 364.182110 | 64.479686 | -0.20% | overlap |
| long_string_1m / stringify | 36.458897 | 35.162851 | 102.200312 | 94.981270 | -3.55% | overlap |
| escaped_1m / parse | 980.164557 | 980.082278 | 1762.753165 | 2073.360759 | -0.01% | overlap |
| escaped_1m / stringify | 885.313253 | 885.150602 | 1897.355422 | 2088.090361 | -0.02% | overlap |
| unicode_1m / parse | 0.348887 | 0.348887 | 441.138191 | 57.795406 | +0.00% | overlap |
| unicode_1m / stringify | 29.067879 | 29.014949 | 409.086465 | 448.588687 | -0.18% | overlap |
| wide_1m / parse | 2883.390625 | 2883.093750 | 5253.265625 | 4140.812500 | -0.01% | overlap |
| wide_1m / stringify | 633.818966 | 633.646552 | 6352.879310 | 663.284483 | -0.03% | overlap |
| heterogeneous_1m / parse | 1116.436620 | 1118.035211 | 3827.457746 | 2949.697183 | +0.14% | overlap |
| heterogeneous_1m / stringify | 717.422018 | 731.527523 | 898.876147 | 1054.990826 | +1.97% | regression |

Full matrix RSS

| Fixture / operation | Main MiB | R23 MiB | Node MiB | Bun MiB | R23 − main MiB |
|---|---:|---:|---:|---:|---:|
| null / parse | 12.812 | 12.828 | 57.703 | 35.750 | +0.016 |
| null / stringify | 13.016 | 13.047 | 59.594 | 129.828 | +0.031 |
| string_a / parse | 12.812 | 12.828 | 57.672 | 36.125 | +0.016 |
| string_a / stringify | 13.016 | 13.031 | 59.641 | 129.812 | +0.016 |
| empty_object / parse | 32.141 | 32.156 | 59.578 | 69.062 | +0.016 |
| empty_object / stringify | 13.109 | 13.125 | 59.656 | 129.828 | +0.016 |
| tiny_object / parse | 32.281 | 32.297 | 59.625 | 69.062 | +0.016 |
| tiny_object / stringify | 32.328 | 32.344 | 59.688 | 129.812 | +0.016 |
| small_record / parse | 79.547 | 79.594 | 59.656 | 79.922 | +0.047 |
| small_record / stringify | 33.344 | 33.359 | 59.750 | 378.719 | +0.016 |
| object_1k / parse | 64.922 | 64.969 | 61.828 | 71.266 | +0.047 |
| object_1k / stringify | 33.359 | 33.391 | 61.828 | 70.891 | +0.031 |
| records_array_16k / parse | 63.438 | 63.453 | 65.859 | 70.609 | +0.016 |
| records_array_16k / stringify | 33.531 | 33.547 | 61.938 | 71.047 | +0.016 |
| records_array_16k / sparse | 72.219 | 72.234 | 66.016 | 70.672 | +0.016 |
| records_array_16k / scan | 204.453 | 204.469 | 61.953 | 77.406 | +0.016 |
| records_array_16k / roundtrip | 69.250 | 68.781 | 65.969 | 70.891 | -0.469 |
| records_array_1m / parse | 67.109 | 67.125 | 125.656 | 88.719 | +0.016 |
| records_array_1m / stringify | 63.016 | 63.031 | 129.406 | 103.703 | +0.016 |
| records_array_1m / sparse | 67.625 | 67.656 | 125.625 | 97.062 | +0.031 |
| records_array_1m / scan | 158.500 | 158.516 | 97.609 | 83.219 | +0.016 |
| records_array_1m / roundtrip | 61.484 | 61.500 | 152.125 | 93.328 | +0.016 |
| records_object_1m / parse | 66.438 | 66.484 | 93.031 | 79.188 | +0.047 |
| records_object_1m / stringify | 63.078 | 63.062 | 129.453 | 103.703 | -0.016 |
| records_array_8m / parse | 108.828 | 108.844 | 266.094 | 140.859 | +0.016 |
| records_array_8m / stringify | 121.141 | 121.172 | 212.500 | 176.844 | +0.031 |
| records_array_8m / sparse | 109.156 | 109.172 | 266.047 | 185.781 | +0.016 |
| records_array_8m / scan | 186.719 | 186.734 | 230.078 | 133.453 | +0.016 |
| records_array_8m / roundtrip | 129.266 | 129.281 | 267.422 | 151.469 | +0.016 |
| records_object_8m / parse | 187.328 | 187.344 | 244.234 | 131.375 | +0.016 |
| records_object_8m / stringify | 121.188 | 121.172 | 212.609 | 196.656 | -0.016 |
| records_array_20m / parse | 255.422 | 255.438 | 359.812 | 220.594 | +0.016 |
| records_array_20m / stringify | 235.234 | 235.234 | 459.516 | 304.906 | +0.000 |
| records_array_20m / sparse | 255.422 | 255.438 | 359.719 | 220.891 | +0.016 |
| records_array_20m / scan | 255.453 | 255.469 | 330.781 | 223.000 | +0.016 |
| records_array_20m / roundtrip | 295.391 | 295.375 | 342.156 | 255.750 | -0.016 |
| records_object_20m / parse | 255.438 | 255.453 | 359.625 | 220.594 | +0.016 |
| records_object_20m / stringify | 235.234 | 235.250 | 459.516 | 304.812 | +0.016 |
| numbers_1m / parse | 62.781 | 62.797 | 104.109 | 68.250 | +0.016 |
| numbers_1m / stringify | 57.844 | 57.859 | 113.109 | 74.266 | +0.016 |
| long_string_1m / parse | 16.438 | 16.469 | 209.172 | 162.469 | +0.031 |
| long_string_1m / stringify | 54.516 | 54.500 | 183.641 | 155.609 | -0.016 |
| escaped_1m / parse | 58.406 | 58.422 | 102.594 | 67.891 | +0.016 |
| escaped_1m / stringify | 54.891 | 54.906 | 124.844 | 74.781 | +0.016 |
| unicode_1m / parse | 15.766 | 15.781 | 204.094 | 156.203 | +0.016 |
| unicode_1m / stringify | 53.859 | 53.875 | 186.281 | 161.125 | +0.016 |
| wide_1m / parse | 227.703 | 227.719 | 121.203 | 95.078 | +0.016 |
| wide_1m / stringify | 68.672 | 68.656 | 118.000 | 84.703 | -0.016 |
| heterogeneous_1m / parse | 62.562 | 62.578 | 92.938 | 99.469 | +0.016 |
| heterogeneous_1m / stringify | 61.719 | 61.734 | 128.984 | 104.938 | +0.016 |

## Independent regression check and sparse-only control

The five rows below use the original work counts with eleven fresh-process repetitions per engine. Every listed slowdown has separated sample ranges and is slower in all eleven main/candidate pairs. The R22 control executable is compiled and linked with its frozen, verified compiler/runtime archives; its benchmark object file is byte-identical to both R23 arms. Its role and old source `8b7ffae96e733c96b31aa41d064902bc4f9a50ce` are explicit in the control archive.

| Fixture / operation | R23 full run | R23 recheck | R22 same-object control |
|---|---:|---:|---:|
| tiny_object / parse | +1.46% | +1.55% | +1.52% |
| records_array_16k / roundtrip | +2.44% | +2.53% | +2.45% |
| records_array_1m / roundtrip | +1.72% | +1.88% | +1.79% |
| records_object_1m / parse | +0.65% | +0.65% | +0.59% |
| heterogeneous_1m / stringify | +1.97% | +1.86% | +1.72% |

This control establishes that those regressions already accompany the sparse-cache change. It does not prove a particular instruction or layout decision caused them. The next proposed experiment outlines the fast lazy accessor to test whether preserving the shared ordinary-array dispatcher’s structure removes them. That experiment is not part of R23.

## Separate follow-up diagnostics

A symbolized executable linked to the exact main runtime reproduces the lazy-array/zero-spacer crash in `write_escaped_bytes_from`, called by `js_json_stringify_full`; the apparent string length is `LAZY_ARRAY_MAGIC`. Source inspection finds missing lazy-array GC-tag cases in compact value/depth/per-element dispatch. An 18-case main-only root/object-wrapper/array-wrapper probe records 14 SIGSEGV outcomes and four Node matches. These overlap the existing failure family; they are not fourteen distinct bugs. The diagnostic executable retains symbols and is not used for performance claims. Replacer/pretty walks also need explicit GC custody checks when materialization introduces allocation.

A separate, standalone scalar prototype compares a saturating u32 conversion plus exact float round-trip against the source-extracted current numeric-index predicate. It matches 15,525,263 tested bit patterns (2,000,477 accepted, 13,524,786 rejected). Source excerpts, hashes, Rust version, flags and assembly are recorded. This prototype is not integrated into the runtime, is not a complete semantic proof, and has no measured JSON speed claim.

## Build and evidence

The exact production command was `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static` on clean committed source, starting 2026-09-11T05:31:51.257171Z and completing in 381.60 seconds. All three emitted artifact mtimes are after the recorded start; frozen copies were hash-verified.

| Artifact | SHA-256 |
|---|---|
| perry | `d65b94cfa8e00628fec37eaa4bb23f34cf3cfe41555bd54df96a75abf403a021` |
| libperry_runtime.a | `ccf7bf35f00058bd2c59e5c45006ce2364678102fb8727e37365412929487cf6` |
| libperry_stdlib.a | `937e0312ae1709d61a4981ea6c05fe163112c7785fcd2f1082afcf1bfa0dcc0f` |

Repair history is preserved: the first R23 unit compilation on 52570a335 was intentionally stopped after the new descriptor test tripped raw-handle debt lint; scoped handle calls fixed it without changing the ratchet. No production build was qualified from that superseded source. The initial candidate-validation controller later stopped on a missing copied fractional-spacing baseline JSON after the preceding checks had passed; its exact inputs were hash-verified, the missing file was restored and only the remaining checks resumed. Final source/tests/build/static/performance records all identify fc877118c.

The four windows are under `results/quiet-materialized-read-r23-*`. The validation archive includes passing and failing outputs, source/build provenance, static findings, disassembly, standalone diagnostics and analyzers; executable/archive/object binaries are represented by hashes rather than committed. `results/materialized-read-r23-artifacts.json` indexes every committed evidence file. Prepared scripts for unexecuted controls are not evidence those controls ran.

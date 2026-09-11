# Bounded dominant-token parser dispatch: R43

The large-token specialization now uses a bounded prefix eligibility check. Across the three qualified windows, 8 of 33 comparisons retain separated CPU regressions against frozen R26. R43 is not promoted and does not establish the no-regression objective.

The experiment includes 1,116 timed trials, 200 complete-output verification records and 84 calibration trials. The targeted screen uses eleven interleaved R26/Perry/Node/Bun repetitions per case; fresh-input and Korean controls use seven. Node is pinned to 26.5.1 and Bun to 1.3.14. CPU is user plus system time per loop iteration; peak RSS covers the complete process, including startup.

Median peak-RSS changes versus R26 range from -80 to +32 KiB. Raw per-repetition vectors and complete-output checks are included in the indexed evidence.

## Implementation and validation

Inputs up to 256 bytes retain the ordinary parser; 257–4,096 bytes retain the length-aware parser. Larger inputs use the length-aware specialization only if the first 512 bytes can contain the start of a dominant unescaped string token. Such a token must open within the first 256 bytes and remain open at byte 512. This necessary-condition check can admit false positives; both parsers still validate the entire document. The check uses the existing quote/backslash scanner, stays outlined for large inputs, and allocates no managed memory. The R42 outlined object-parser boundary remains in place.

Tests extend the source-length boundaries through 4,095/4,096/4,097/4,098/8,192 bytes, sweep valid dominant-token prefix/suffix positions, compare full outputs for wide objects, short-record arrays and Unicode roots, and reject malformed trailing syntax through both dispatch paths.

R43 source 9415ce52ec36e72d643a70b0833240b407ea31d1: 314 serial JSON unit tests pass in 2.97 s and the broad string filter passes 385 tests in 4.64 s (overlapping suites). A normal cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static completes in 407.619 s; immutable compiler/runtime/stdlib hashes and both source trees are verified. All 125 canonical candidate full-output executions pass: 46 fixtures, 14 options, nine zero-spacing, 20 emitter, four primitive-key, 20 large-emitter, four changing-options and eight retained-output checks. Scheduled cases assert copying and protected retired pages. Thirty emitted IR files match the frozen R26 reference after only the existing native ModuleID-comment normalization; all six worker object files match exactly. Original-suite static results reuse actual R39 checker executions only after scripts-tree, fixture, log and exact IR dependency verification. Additional static checks are fresh. Known native-root findings remain unsuppressed; existing lazy-descriptor, fractional-spacing and getter findings are compared explicitly rather than classified as passes. All implementation lint audits and format/file-cap checks pass; 72/74 lint gates pass. Public-baseline freshness and inherited raw-handle ceiling debt remain unresolved. The explicit raw-handle comparison is pinned to f6c6879613c0b00b5e3ae873b8d102710d2d0585 and matches R42 output byte for byte; the full lint ran after origin/main advanced to 435d6396. The build retains the known relevant_box_roots dead-code warning. No GC core, policy, thresholds, parse-boundary hooks, cache admission or size-cap changes.

Known native-root findings, lazy-descriptor/getter/fractional-spacing findings and frozen-reference emitter failures remain explicit. Passing candidate checks do not erase those findings or establish full conformance.

## Targeted repeated-input screen

Window: 2026-09-11T18:41:07Z to 2026-09-11T18:43:58Z. 528 timed, 60 verification and 0 calibration records.

| Fixture / operation | R26 µs | R43 µs | Node µs | Bun µs | Change vs R26 | Screen |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / sparse | 20.043594 | 20.190264 | 40.304016 | 33.956660 | +0.73% | Overlap |
| records_object_1m / parse | 2009.197531 | 2010.000000 | 2821.160494 | 2149.320988 | +0.04% | Overlap |
| records_object_8m / parse | 15909.700000 | 15731.500000 | 34612.600000 | 20876.800000 | -1.12% | Overlap |
| records_array_20m / stringify | 11786.416667 | 11867.750000 | 17324.750000 | 20138.500000 | +0.69% | Overlap |
| long_string_1m / stringify | 34.873049 | 34.234651 | 102.100937 | 94.825182 | -1.83% | Overlap |
| unicode_1m / stringify | 28.207677 | 27.966061 | 409.048485 | 448.798384 | -0.86% | Overlap |
| wide_1m / parse | 2888.703125 | 3018.250000 | 5266.328125 | 4129.875000 | +4.48% | Regression |
| wide_1m / stringify | 633.642241 | 648.142241 | 6353.974138 | 663.564655 | +2.29% | Regression |
| heterogeneous_1m / parse | 1116.253521 | 1123.556338 | 3831.394366 | 2974.225352 | +0.65% | Regression |
| tiny_object / parse | 0.036776 | 0.037076 | 0.080876 | 0.045158 | +0.82% | Overlap |
| records_object_20m / stringify | 11710.416667 | 11786.250000 | 17303.000000 | 20088.500000 | +0.65% | Regression |
| numbers_1m / stringify | 1014.326531 | 1019.959184 | 1975.197279 | 2943.115646 | +0.56% | Regression |

| Fixture / operation | R26 MiB | R43 MiB | Node MiB | Bun MiB |
|---|---:|---:|---:|---:|
| records_array_16k / sparse | 72.234 | 72.203 | 65.984 | 70.656 |
| records_object_1m / parse | 66.469 | 66.391 | 92.969 | 79.188 |
| records_object_8m / parse | 187.328 | 187.281 | 244.344 | 131.250 |
| records_array_20m / stringify | 235.219 | 235.188 | 459.531 | 304.953 |
| long_string_1m / stringify | 54.484 | 54.453 | 183.625 | 155.625 |
| unicode_1m / stringify | 53.875 | 53.844 | 186.359 | 154.000 |
| wide_1m / parse | 227.703 | 227.672 | 121.156 | 95.094 |
| wide_1m / stringify | 68.672 | 68.641 | 117.938 | 84.641 |
| heterogeneous_1m / parse | 62.562 | 62.547 | 92.969 | 99.703 |
| tiny_object / parse | 32.281 | 32.250 | 59.625 | 69.094 |
| records_object_20m / stringify | 235.234 | 235.203 | 459.516 | 305.000 |
| numbers_1m / stringify | 57.844 | 57.828 | 113.125 | 74.297 |

## Fresh-input and repeated-input controls

Window: 2026-09-11T18:45:01Z to 2026-09-11T18:46:43Z. 420 timed, 100 verification and 60 calibration records.

| Fixture / operation | R26 µs | R43 µs | Node µs | Bun µs | Change vs R26 | Screen |
|---|---:|---:|---:|---:|---:|---|
| small_record / rotating | 0.494343 | 0.480051 | 0.343276 | 0.254823 | -2.89% | Gain |
| small_record / same | 0.099067 | 0.099349 | 0.329981 | 0.246475 | +0.29% | Overlap |
| small_record / select | 0.009441 | 0.009567 | 0.002677 | 0.003655 | +1.34% | Overlap |
| records_array_1m / rotating | 940.062147 | 924.807910 | 2689.129944 | 2147.898305 | -1.62% | Gain |
| records_array_1m / same | 939.182857 | 922.028571 | 2650.342857 | 2146.891429 | -1.83% | Gain |
| records_array_1m / select | 0.011225 | 0.011345 | 0.003092 | 0.003873 | +1.07% | Overlap |
| long_string_1m / rotating | 99.681015 | 97.838586 | 372.371253 | 69.376633 | -1.85% | Gain |
| long_string_1m / same | 0.359405 | 0.384939 | 368.638332 | 65.920168 | +7.10% | Regression |
| long_string_1m / select | 0.011558 | 0.011601 | 0.003102 | 0.003879 | +0.37% | Overlap |
| escaped_1m / rotating | 1007.833333 | 951.279762 | 1724.809524 | 2077.595238 | -5.61% | Gain |
| escaped_1m / same | 990.609467 | 932.550296 | 1723.804734 | 2073.313609 | -5.86% | Gain |
| escaped_1m / select | 0.011704 | 0.011375 | 0.003141 | 0.003861 | -2.81% | Overlap |
| unicode_1m / rotating | 141.810458 | 84.246187 | 443.773420 | 63.931736 | -40.59% | Gain |
| unicode_1m / same | 0.358140 | 0.383184 | 437.287657 | 58.989982 | +6.99% | Regression |
| unicode_1m / select | 0.011410 | 0.011400 | 0.003159 | 0.003889 | -0.08% | Overlap |

| Fixture / operation | R26 MiB | R43 MiB | Node MiB | Bun MiB |
|---|---:|---:|---:|---:|
| small_record / rotating | 31.984 | 31.938 | 59.953 | 80.219 |
| small_record / same | 76.781 | 76.750 | 59.656 | 79.875 |
| small_record / select | 12.812 | 12.844 | 57.844 | 35.281 |
| records_array_1m / rotating | 76.297 | 76.281 | 128.922 | 98.297 |
| records_array_1m / same | 76.297 | 76.281 | 128.875 | 98.297 |
| records_array_1m / select | 22.594 | 22.609 | 67.609 | 42.531 |
| long_string_1m / rotating | 90.234 | 90.172 | 177.625 | 123.516 |
| long_string_1m / same | 24.594 | 24.609 | 198.266 | 150.578 |
| long_string_1m / select | 24.328 | 24.328 | 68.984 | 43.781 |
| escaped_1m / rotating | 65.000 | 65.000 | 89.609 | 77.750 |
| escaped_1m / same | 65.000 | 65.000 | 90.359 | 77.781 |
| escaped_1m / select | 23.531 | 23.547 | 68.406 | 43.266 |
| unicode_1m / rotating | 61.562 | 61.500 | 158.578 | 155.281 |
| unicode_1m / same | 22.703 | 22.719 | 212.203 | 132.094 |
| unicode_1m / select | 22.438 | 22.453 | 68.609 | 44.641 |

## Korean input controls

Window: 2026-09-11T18:47:25Z to 2026-09-11T18:48:07Z. 168 timed, 40 verification and 24 calibration records.

| Fixture / operation | R26 µs | R43 µs | Node µs | Bun µs | Change vs R26 | Screen |
|---|---:|---:|---:|---:|---:|---|
| korean_small / rotating | 0.518383 | 0.477149 | 0.401489 | 0.213754 | -7.95% | Gain |
| korean_small / same | 0.082940 | 0.082995 | 0.391729 | 0.204072 | +0.07% | Overlap |
| korean_small / select | 0.009501 | 0.009435 | 0.002670 | 0.003629 | -0.68% | Overlap |
| korean_1m / rotating | 164.995185 | 133.927234 | 243.683788 | 45.883360 | -18.83% | Gain |
| korean_1m / same | 0.356735 | 0.382346 | 247.589527 | 43.237366 | +7.18% | Regression |
| korean_1m / select | 0.011451 | 0.011869 | 0.003093 | 0.003864 | +3.65% | Overlap |

| Fixture / operation | R26 MiB | R43 MiB | Node MiB | Bun MiB |
|---|---:|---:|---:|---:|
| korean_small / rotating | 32.281 | 32.297 | 59.953 | 70.641 |
| korean_small / same | 64.922 | 64.922 | 61.750 | 70.391 |
| korean_small / select | 12.812 | 12.844 | 57.891 | 35.344 |
| korean_1m / rotating | 90.578 | 90.531 | 149.781 | 119.406 |
| korean_1m / same | 24.656 | 24.672 | 190.828 | 133.469 |
| korean_1m / select | 24.328 | 24.344 | 66.469 | 43.109 |

“Separated” means the smallest candidate sample exceeds the largest reference sample, or vice versa for an improvement. It is a descriptive screen, not a significance test; overlapping ranges do not prove equivalence. Fresh-input controls rotate eight separately allocated inputs with differing contents; they are separate workloads from repeatedly parsing one unchanged source.

## Immediate parent observations

R42 and R43 below are separate quiet windows, not interleaved parent/candidate pairs. Both use eleven repetitions and the same twelve declared cases. These historical R42 vectors add no R43 trial counts.

| Fixture / operation | R42 µs | R43 µs | Difference |
|---|---:|---:|---:|
| records_array_16k / sparse | 20.106762 | 20.190264 | +0.42% |
| records_object_1m / parse | 2027.493827 | 2010.000000 | -0.86% |
| records_object_8m / parse | 16111.400000 | 15731.500000 | -2.36% |
| records_array_20m / stringify | 11829.083333 | 11867.750000 | +0.33% |
| long_string_1m / stringify | 34.323621 | 34.234651 | -0.26% |
| unicode_1m / stringify | 28.824242 | 27.966061 | -2.98% |
| wide_1m / parse | 2977.062500 | 3018.250000 | +1.38% |
| wide_1m / stringify | 648.396552 | 648.142241 | -0.04% |
| heterogeneous_1m / parse | 1124.492958 | 1123.556338 | -0.08% |
| tiny_object / parse | 0.037073 | 0.037076 | +0.01% |
| records_object_20m / stringify | 11789.083333 | 11786.250000 | -0.02% |
| numbers_1m / stringify | 1019.836735 | 1019.959184 | +0.01% |

## Provenance and limits

Candidate source: `9415ce52ec36e72d643a70b0833240b407ea31d1`. Immediate parent: `b9bfe3070f90f223e1d9d3526df54beb57308328`. Frozen R26 reference: `3aac4d6335da54abeeed73df842decbbe6dd5d71`. Build artifacts, source patches, fixture hashes, command exits, raw samples and outputs are recorded. All 150 original remote staging hashes and the additional Korean assets are verified. Every terminal timing window is archived before subsequent remote operations.

Initial controller launches occurred before local bootstrap finished and exited 2 without running Cargo or lint. A separate local staging attempt later refused a stale receipt filename before contacting the benchmark machine. It was corrected to read the actual successful canonical-validation receipt; the original failed scripts/logs/exit codes remain archived. These setup errors are not runtime/test failures. The deliberate post-unit production hold is also preserved separately from the successful test exit.

No R43 full-50, stringify-options, retained-output or access-specific timing window ran. The earlier R41 packed-escaping options measurements and R32 integer-remainder access gains belong to their respective sources; R32 is outside this experimental lineage. Current main was not benchmarked. No CI waiting or administrative merge is involved.

Next investigation: refine the outlined scanner continuation used for ordinary Korean UTF-8 after a valid ED prefix. Its standalone prototype is not a runtime performance result. Parser/stringifier regressions still require correction and integration before a ready PR can carry the combined experimental changes.

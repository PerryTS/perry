# Fresh measurements of merged main e7223f700

Main `e7223f700c8ce69c388210dab394ad7142550527` (0.5.1528) was freshly built with the matched compiler and both static wrappers after #10033 incorporated #10032. These are measurements of that merge, not a relabeling of the earlier PR binaries. The original workload objects are identical in both runtime arms; build and worker hashes are recorded in the result directories.

Apple M1, 8 GiB RAM; Node26.5.1, Bun1.3.14; five repetitions per engine and equal work per row. CPU is process CPU per call. RSS is whole-process memory, including runtime and input. Both windows passed the load/competing-workload gate. Default GC settings are used for standings.

## Parse and stringify suite

Main leads **46/50 CPU rows, 67/86 peak-RSS rows and 31/36 retained-current-RSS rows**. All 200 output comparisons and 344 measurement groups validate. Repeated-source parsing can benefit from existing source reuse. Large-array full scans are separate from lazy parse and untouched roundtrip.

All engine medians, including the complete retained-memory matrix, are in [the full comparison](results/quiet-decoder-r2-main-all-r5/comparison.md). In that table **baseline means merged main**, and perry means the pending #10034 correction.

Main CPU rows still above the fastest competing engine:

| Fixture | Operation | Main us | Node us | Bun us | Main / best |
|---|---|---:|---:|---:|---:|
| records_array_16k | scan | 63.284 | 41.148 | 35.636 | 1.776x |
| records_array_1m | scan | 3253.926 | 2888.574 | 2223.691 | 1.463x |
| records_array_20m | roundtrip | 99398.500 | 103757.000 | 77047.000 | 1.290x |
| records_array_8m | scan | 25183.000 | 30477.750 | 21411.125 | 1.176x |

## Changing input

Eight equal-size, same-shape sources are preloaded for every engine. Seventeen fixtures change one value; null and the empty object have unchanged contents in separately loaded strings. The same-source and selection-only controls retain that identical pool. Selection costs are reported separately without subtraction. This avoids the single-source reuse case, but is not an unbounded stream of unique shapes. Array parse can still defer materialization; use the full-scan rows above for complete traversal.

All 380 output comparisons and 1140 trials validate. The table below shows **all 19 changing-input parse rows for main**. Peak RSS includes the eight-input pool.

| Fixture | Main CPU us | Node us | Bun us | Main / best | Peak MiB: main / Node / Bun |
|---|---:|---:|---:|---:|---:|
| null | 0.015643 | 0.027637 | 0.019929 | 0.785x | 12.84 / 57.92 / 35.44 |
| string_a | 0.020526 | 0.031996 | 0.023766 | 0.864x | 12.86 / 57.91 / 36.34 |
| empty_object | 0.030381 | 0.045542 | 0.025455 | 1.194x | 32.22 / 59.89 / 69.31 |
| tiny_object | 0.046975 | 0.081895 | 0.046955 | 1.000x | 32.33 / 59.91 / 69.27 |
| small_record | 0.498190 | 0.345953 | 0.254872 | 1.955x | 32.02 / 59.84 / 80.17 |
| object_1k | 0.498152 | 0.541125 | 0.240503 | 2.071x | 32.38 / 62.00 / 71.41 |
| records_array_16k | 18.321065 | 40.610414 | 33.951834 | 0.540x | 63.64 / 66.17 / 70.52 |
| records_array_1m | 1213.413534 | 2791.699248 | 2158.834586 | 0.562x | 76.33 / 128.80 / 93.72 |
| records_object_1m | 2052.114286 | 2843.171429 | 2172.257143 | 0.945x | 66.19 / 96.08 / 85.52 |
| records_array_8m | 10173.142857 | 33339.857143 | 18889.285714 | 0.539x | 174.02 / 335.41 / 227.52 |
| records_object_8m | 16056.333333 | 35819.333333 | 20725.777778 | 0.775x | 303.77 / 290.16 / 228.88 |
| records_array_20m | 43068.125000 | 77348.625000 | 46913.375000 | 0.918x | 729.17 / 610.42 / 473.12 |
| records_object_20m | 43223.375000 | 79520.750000 | 47070.500000 | 0.918x | 729.17 / 611.53 / 473.66 |
| numbers_1m | 1577.433735 | 3144.879518 | 3218.240964 | 0.502x | 67.00 / 111.66 / 77.41 |
| long_string_1m | 107.846799 | 372.543445 | 68.899390 | 1.565x | 90.28 / 177.50 / 123.44 |
| escaped_1m | 1001.430380 | 1727.512658 | 2076.924051 | 0.580x | 65.05 / 89.67 / 77.75 |
| unicode_1m | 147.918498 | 438.653437 | 62.687456 | 2.360x | 62.42 / 159.55 / 153.39 |
| wide_1m | 2678.234375 | 5443.390625 | 4166.328125 | 0.643x | 252.33 / 145.86 / 97.78 |
| heterogeneous_1m | 1423.977528 | 3828.123596 | 2970.000000 | 0.479x | 85.89 / 97.27 / 92.09 |

[All changing-input, same-source and selection controls, with current RSS](results/quiet-decoder-r2-main-rotating-r5/comparison.md).

## Escaped-record correction

Main retains a confirmed correctness defect: isolated tape reads can discard lone-surrogate escapes, and generic stringify can rename a lone-surrogate key. [PR10034](https://github.com/PerryTS/perry/pull/10034) shares the canonical decoder and fixes the key writer. Its 283 JSON tests, 14 compiled Node comparisons, and moving/protected/full-GC lifetime checks pass. Twelve compiled cases fail on this rebuilt main and pass on the correction.

The first correction had a reproducible sparse-read slowdown and was rejected. Restoring the scanner call boundary removes it: the longer paired comparison is +0.063% vs main with overlapping samples. In the full original suite the largest slower CPU median is +0.315%, also overlapping, and process RSS differences are at most 80 KiB. See [the correction report](ESCAPED_RECORD_CORRECTION.md) for final changing-input deltas and evidence.

## Next work

The parity goal remains open. Fresh small-object parsing, large strings, full scans and several large-object memory rows still have gaps. [Fresh stack samples](results/main-e722-location-profiles/README.md) identify redundant UTF-16 counting and nesting/token scans, plus small-record boundary and construction costs. The separate source-length experiment is not part of main or the correction; it needs its small-object cost resolved and fresh validation before acceptance.

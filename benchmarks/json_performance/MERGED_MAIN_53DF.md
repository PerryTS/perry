# Fresh merged-main JSON measurements: 53df2c671

Main `53df2c671fff33b1f8f624432372c2401db8b1d0` (0.5.1530) contains the accepted JSON work landed through train #10037. Its complete tree matches final PR #10036 head `6a5d2ba5e`. The matching compiler and static libraries were rebuilt from that actual commit, and fresh workers were linked. [Build, source identity and compiled GC validation](results/main-53df-validation/README.md).

Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Five repetitions per engine and equal work within each row, under qualified quiet-host windows. CPU is process CPU inside the measured loop per call; RSS includes runtime and inputs. Results use medians; a lower median alone does not establish statistical significance. The `perry` arm is newly built main53df and `baseline` is earlier main eee3881c4.

## Original suite

Main leads both engines on 46/50 CPU medians, 66/86 peak-RSS medians and 31/36 retained-current-RSS medians. All 200 output checks and 344 measurement groups validate. An additional checksum/count audit covers all 2120 verification, calibration, timing and memory records.

[Every original CPU, peak-RSS and current-RSS row](results/quiet-main-53df-all-r5/comparison.md). Repeated-source parse can reuse source/template information. The 38 parse/stringify CPU rows follow; array consumption is listed separately.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.010637 | 0.027355 | 0.019163 | 0.555x |
| null | stringify | 0.006569 | 0.027193 | 0.028611 | 0.242x |
| string_a | parse | 0.012825 | 0.031740 | 0.022565 | 0.568x |
| string_a | stringify | 0.009690 | 0.030026 | 0.029358 | 0.330x |
| empty_object | parse | 0.023399 | 0.045228 | 0.024123 | 0.970x |
| empty_object | stringify | 0.017611 | 0.031975 | 0.029821 | 0.591x |
| tiny_object | parse | 0.036769 | 0.081066 | 0.045216 | 0.813x |
| tiny_object | stringify | 0.033978 | 0.037171 | 0.039507 | 0.914x |
| small_record | parse | 0.098678 | 0.335758 | 0.244089 | 0.404x |
| small_record | stringify | 0.045699 | 0.108149 | 0.119467 | 0.423x |
| object_1k | parse | 0.081945 | 0.531703 | 0.227185 | 0.361x |
| object_1k | stringify | 0.115010 | 0.193664 | 0.211130 | 0.594x |
| records_array_16k | parse | 13.704981 | 40.959146 | 33.880362 | 0.405x |
| records_array_16k | stringify | 11.024481 | 13.137848 | 23.211522 | 0.839x |
| records_array_1m | parse | 922.876471 | 2707.547059 | 2145.735294 | 0.430x |
| records_array_1m | stringify | 648.519651 | 849.493450 | 962.139738 | 0.763x |
| records_object_1m | parse | 2016.086420 | 2840.666667 | 2159.530864 | 0.934x |
| records_object_1m | stringify | 650.135371 | 847.048035 | 964.991266 | 0.768x |
| records_array_8m | parse | 8303.187500 | 33776.812500 | 20741.687500 | 0.400x |
| records_array_8m | stringify | 4734.551724 | 7217.206897 | 8166.068966 | 0.656x |
| records_object_8m | parse | 15952.300000 | 33459.800000 | 20983.800000 | 0.760x |
| records_object_8m | stringify | 4765.931034 | 7179.482759 | 8139.827586 | 0.664x |
| records_array_20m | parse | 39463.250000 | 93652.750000 | 53175.000000 | 0.742x |
| records_array_20m | stringify | 11743.666667 | 17235.666667 | 20187.333333 | 0.681x |
| records_object_20m | parse | 39528.000000 | 94009.500000 | 53168.000000 | 0.743x |
| records_object_20m | stringify | 11710.500000 | 17300.750000 | 20087.416667 | 0.677x |
| numbers_1m | parse | 1551.274510 | 3124.980392 | 3204.843137 | 0.496x |
| numbers_1m | stringify | 1013.605442 | 1972.965986 | 2946.489796 | 0.514x |
| long_string_1m | parse | 0.351033 | 364.734854 | 64.443336 | 0.005x |
| long_string_1m | stringify | 34.402706 | 102.186264 | 95.100416 | 0.362x |
| escaped_1m | parse | 980.158228 | 1761.968354 | 2073.284810 | 0.556x |
| escaped_1m | stringify | 884.584337 | 1895.048193 | 2083.500000 | 0.467x |
| unicode_1m | parse | 0.349605 | 442.076095 | 57.961953 | 0.006x |
| unicode_1m | stringify | 27.833131 | 409.977778 | 448.834747 | 0.068x |
| wide_1m | parse | 2893.406250 | 5260.312500 | 4143.343750 | 0.698x |
| wide_1m | stringify | 633.586207 | 6379.685345 | 664.512931 | 0.953x |
| heterogeneous_1m | parse | 1121.119718 | 3826.725352 | 2948.288732 | 0.380x |
| heterogeneous_1m | stringify | 718.261468 | 896.408257 | 1055.591743 | 0.801x |

## Array consumption

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| records_array_16k | sparse | 20.025 | 39.744 | 33.998 | 0.589x |
| records_array_16k | scan | 58.657 | 39.350 | 35.697 | 1.643x |
| records_array_16k | roundtrip | 21.098 | 52.408 | 57.810 | 0.403x |
| records_array_1m | sparse | 969.484 | 2690.106 | 2186.634 | 0.443x |
| records_array_1m | scan | 2938.696 | 2841.681 | 2227.957 | 1.319x |
| records_array_1m | roundtrip | 1412.104 | 3527.148 | 3110.530 | 0.454x |
| records_array_8m | sparse | 8920.267 | 33160.800 | 21556.667 | 0.414x |
| records_array_8m | scan | 22672.625 | 29538.375 | 21483.125 | 1.055x |
| records_array_8m | roundtrip | 13173.455 | 36578.000 | 27875.727 | 0.473x |
| records_array_20m | sparse | 39469.000 | 94199.500 | 53174.750 | 0.742x |
| records_array_20m | scan | 40467.000 | 91314.500 | 53533.250 | 0.756x |
| records_array_20m | roundtrip | 98653.500 | 100507.500 | 76629.000 | 1.287x |

## Changing input

Eight equal-size, same-shape sources are preloaded. Seventeen fixtures change a value; null and the empty object have identical contents in separately loaded strings. Same-source and selection-only controls keep the same pool alive. Selection costs are reported without subtraction. All 380 output checks and 1140 timing trials validate.

[All changing-input, same-source and selection-control CPU/RSS rows](results/quiet-main-53df-rotating-r5/comparison.md).

| Fixture | Perry µs | Node µs | Bun µs | Perry / best | Peak MiB: Perry / Node / Bun |
|---|---:|---:|---:|---:|---:|
| null | 0.015633 | 0.027635 | 0.019925 | 0.785x | 12.83 / 57.88 / 35.44 |
| string_a | 0.020470 | 0.032010 | 0.023801 | 0.860x | 12.83 / 58.00 / 36.38 |
| empty_object | 0.029824 | 0.045527 | 0.025381 | 1.175x | 32.14 / 59.84 / 69.30 |
| tiny_object | 0.046909 | 0.082621 | 0.046747 | 1.003x | 32.27 / 59.91 / 69.30 |
| small_record | 0.494699 | 0.341584 | 0.255293 | 1.938x | 31.98 / 59.91 / 80.16 |
| object_1k | 0.482649 | 0.540965 | 0.240771 | 2.005x | 32.33 / 61.92 / 71.41 |
| records_array_16k | 13.964088 | 41.548581 | 33.886645 | 0.412x | 63.61 / 66.19 / 70.56 |
| records_array_1m | 942.965517 | 2790.770115 | 2153.419540 | 0.438x | 76.31 / 128.72 / 98.25 |
| records_object_1m | 2032.757143 | 2883.071429 | 2169.385714 | 0.937x | 66.12 / 96.09 / 85.53 |
| records_array_8m | 7895.111111 | 33297.277778 | 18969.722222 | 0.416x | 173.97 / 374.52 / 227.42 |
| records_object_8m | 15828.555556 | 35553.777778 | 20565.000000 | 0.770x | 303.72 / 290.06 / 229.58 |
| records_array_20m | 42635.500000 | 79806.000000 | 47115.750000 | 0.905x | 729.12 / 610.59 / 474.39 |
| records_object_20m | 42686.625000 | 79125.750000 | 47128.625000 | 0.906x | 729.12 / 683.78 / 474.38 |
| numbers_1m | 1553.452381 | 3141.976190 | 3219.500000 | 0.494x | 66.95 / 111.70 / 77.41 |
| long_string_1m | 99.748080 | 372.171275 | 70.209677 | 1.421x | 90.23 / 177.56 / 150.42 |
| escaped_1m | 1011.113208 | 1725.622642 | 2077.742138 | 0.586x | 65.00 / 89.58 / 77.73 |
| unicode_1m | 141.394755 | 438.569100 | 62.942594 | 2.246x | 62.38 / 159.41 / 157.02 |
| wide_1m | 2674.937500 | 5458.390625 | 4160.937500 | 0.643x | 252.25 / 145.86 / 97.73 |
| heterogeneous_1m | 1134.754717 | 3901.849057 | 2986.584906 | 0.380x | 85.86 / 97.33 / 95.38 |

CPU medians lead both engines in 13/19 changing-input rows and 18/19 same-source control rows. Peak RSS leads in 15/19 and 13/19 respectively. Perry trails the selection-only CPU control in all 19 rows; those controls measure input selection, with no JSON parsing. Their absolute costs are available in the complete table.

## Remaining work

The complete parity goal remains open. The four original CPU gaps are the 13 KiB, 1 MiB and 8 MiB array scans plus the 20 MiB roundtrip. Changing-input and large-container memory gaps remain visible above. Positive CPU/RSS deltas versus older main are retained in the raw and comparison tables; these measurements do not prove an absence of all regressions.

Inherited lazy stringify canonicalization failures are documented in [LAZY_CANONICAL.md](LAZY_CANONICAL.md). The slower R4/R5 corrections were parked separately and are not in this main build. A [fresh scan profile](https://github.com/PerryTS/perry/blob/98bd51eb0/benchmarks/json_performance/results/pr-lazy-record-scan-profile/README.md) identifies the adaptive full reparse as a next target; indexed scalar projection is still unimplemented. [Large-container memory diagnostics](GC_MEMORY_GROWTH.md) remain relevant. No collector-policy change is included here.

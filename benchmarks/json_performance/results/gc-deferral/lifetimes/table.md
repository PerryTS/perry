# Supplemental lifetime comparisons

Each row has three matched fresh-process pairs. Total CPU includes the loop and a final explicit collection. API timing includes per-call clocks; these rows are separate from the fixed 38-row baseline. Peak RSS is whole-process, including setup and post-timing verification. “Separated” means the observed total-CPU ranges do not overlap; it is not a statistical confidence interval.

| Fixture | Operation | Lifetime | Calls | API µs parent → candidate | Total CPU change | Separated | Peak RSS change MiB |
|---|---|---|---:|---:|---:|---|---:|
| object_1k | parse | discard | 60000 | 0.503 → 0.505 | +0.19% | False | -0.016 |
| object_1k | parse | latest | 60000 | 0.503 → 0.505 | +0.30% | False | -0.031 |
| object_1k | parse | retain | 20000 | 0.560 → 0.564 | +1.12% | True | +0.000 |
| object_1k | roundtrip | discard | 60000 | 0.696 → 0.698 | +1.53% | False | +0.047 |
| object_1k | stringify | discard | 60000 | 0.203 → 0.204 | +1.29% | True | +0.141 |
| object_1k | stringify | latest | 60000 | 0.203 → 0.205 | +0.27% | False | -0.094 |
| object_1k | stringify | retain | 20000 | 0.245 → 0.247 | +0.47% | False | -0.109 |
| records_object_1m | parse | discard | 120 | 2493.666 → 2431.098 | -1.89% | True | +2.188 |
| records_object_1m | parse | latest | 120 | 2506.678 → 2444.997 | -3.70% | True | +2.562 |
| records_object_1m | parse | retain | 16 | 2612.159 → 2560.323 | -28.65% | True | -8.828 |
| records_object_1m | roundtrip | discard | 120 | 3988.671 → 4074.109 | +0.77% | True | +4.203 |
| records_object_1m | stringify | discard | 120 | 1500.501 → 1510.989 | +0.69% | True | +0.047 |
| records_object_1m | stringify | latest | 120 | 1503.323 → 1509.810 | +0.50% | True | +0.031 |
| records_object_1m | stringify | retain | 16 | 1391.206 → 1396.612 | +0.64% | True | +0.031 |
| records_object_20m | parse | discard | 8 | 73939.875 → 72859.958 | -0.83% | True | -0.016 |
| records_object_20m | parse | latest | 8 | 73787.505 → 72764.021 | -0.59% | True | -1.219 |
| records_object_20m | parse | retain | 2 | 90585.562 → 89928.416 | -0.46% | True | +0.047 |
| records_object_20m | roundtrip | discard | 8 | 103684.151 → 104742.995 | +4.46% | True | -21.062 |
| records_object_20m | stringify | discard | 8 | 27701.854 → 27559.677 | -0.15% | False | -0.016 |
| records_object_20m | stringify | latest | 8 | 27667.266 → 27531.563 | -0.34% | True | -0.031 |
| records_object_20m | stringify | retain | 2 | 27115.771 → 26988.312 | -0.41% | False | +0.000 |
| records_object_8m | parse | discard | 20 | 20043.317 → 19689.169 | -0.68% | True | +0.016 |
| records_object_8m | parse | latest | 20 | 20035.404 → 19609.683 | -0.46% | True | +0.016 |
| records_object_8m | parse | retain | 2 | 19571.938 → 19199.604 | -0.58% | True | +0.031 |
| records_object_8m | roundtrip | discard | 20 | 31130.388 → 30754.002 | -0.40% | True | -3.453 |
| records_object_8m | stringify | discard | 20 | 12907.360 → 12900.337 | -0.08% | False | +0.000 |
| records_object_8m | stringify | latest | 20 | 12881.381 → 12917.613 | +0.23% | False | -0.016 |
| records_object_8m | stringify | retain | 2 | 10913.730 → 10887.750 | -0.03% | False | +0.016 |
| small_record | parse | discard | 100000 | 0.516 → 0.508 | -0.85% | True | -0.016 |
| small_record | parse | latest | 100000 | 0.518 → 0.512 | -0.68% | True | -0.016 |
| small_record | parse | retain | 100000 | 0.525 → 0.520 | -0.16% | False | +0.000 |
| small_record | roundtrip | discard | 100000 | 0.815 → 0.809 | -0.63% | True | -0.094 |
| small_record | stringify | discard | 100000 | 0.287 → 0.301 | +3.44% | True | +0.031 |
| small_record | stringify | latest | 100000 | 0.287 → 0.299 | +2.63% | True | +0.016 |
| small_record | stringify | retain | 100000 | 0.289 → 0.299 | +2.27% | True | +0.047 |
| wide_1m | parse | discard | 60 | 10488.068 → 10421.426 | -43.24% | True | -61.203 |
| wide_1m | parse | latest | 60 | 10475.885 → 10396.292 | -43.86% | True | -45.703 |
| wide_1m | parse | retain | 8 | 10494.573 → 10492.677 | -0.11% | False | +0.094 |
| wide_1m | roundtrip | discard | 60 | 13204.592 → 13173.322 | -17.91% | True | +20.844 |
| wide_1m | stringify | discard | 60 | 1647.363 → 1645.742 | -0.20% | False | +0.062 |
| wide_1m | stringify | latest | 60 | 1648.328 → 1647.728 | -0.10% | False | +0.062 |
| wide_1m | stringify | retain | 8 | 1544.932 → 1548.115 | +0.38% | False | +0.094 |

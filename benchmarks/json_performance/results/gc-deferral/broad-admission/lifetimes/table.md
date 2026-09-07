# Supplemental lifetime comparisons

Each row has three matched fresh-process pairs. Total CPU includes the loop and a final explicit collection. API timing includes per-call clocks; these rows are separate from the fixed 38-row baseline. Peak RSS is whole-process, including setup and post-timing verification. “Separated” means the observed total-CPU ranges do not overlap; it is not a statistical confidence interval.

| Fixture | Operation | Lifetime | Calls | API µs parent → candidate | Total CPU change | Separated | Peak RSS change MiB |
|---|---|---|---:|---:|---:|---|---:|
| object_1k | parse | discard | 60000 | 0.503 → 0.508 | +0.70% | True | +0.078 |
| object_1k | parse | latest | 60000 | 0.504 → 0.514 | +5.78% | False | +0.062 |
| object_1k | parse | retain | 20000 | 0.561 → 0.574 | +1.75% | False | +0.094 |
| object_1k | roundtrip | discard | 60000 | 0.694 → 0.701 | +0.10% | False | +0.188 |
| object_1k | stringify | discard | 60000 | 0.204 → 0.204 | -0.08% | False | +0.156 |
| object_1k | stringify | latest | 60000 | 0.204 → 0.203 | -0.45% | False | -0.016 |
| object_1k | stringify | retain | 20000 | 0.247 → 0.244 | -0.08% | False | +0.016 |
| records_object_1m | parse | discard | 120 | 2493.133 → 2445.975 | -1.30% | True | +2.281 |
| records_object_1m | parse | latest | 120 | 2507.490 → 2466.836 | -2.94% | True | +2.672 |
| records_object_1m | parse | retain | 16 | 2603.435 → 2562.898 | -28.52% | True | -6.531 |
| records_object_1m | roundtrip | discard | 120 | 3989.412 → 4086.693 | +0.89% | True | +3.672 |
| records_object_1m | stringify | discard | 120 | 1500.051 → 1503.082 | +0.23% | True | +0.125 |
| records_object_1m | stringify | latest | 120 | 1500.638 → 1506.008 | +0.34% | False | +0.109 |
| records_object_1m | stringify | retain | 16 | 1394.042 → 1399.719 | +0.26% | False | +0.125 |
| records_object_20m | parse | discard | 8 | 73887.516 → 48440.349 | -9.11% | True | +45.969 |
| records_object_20m | parse | latest | 8 | 73906.208 → 48542.250 | -8.53% | True | +36.969 |
| records_object_20m | parse | retain | 2 | 90745.188 → 47827.125 | -7.65% | True | +2.219 |
| records_object_20m | roundtrip | discard | 8 | 103984.839 → 103396.667 | -0.40% | True | +0.109 |
| records_object_20m | stringify | discard | 8 | 27583.729 → 27573.219 | +0.03% | False | +0.125 |
| records_object_20m | stringify | latest | 8 | 27641.531 → 27653.578 | +0.19% | False | +0.094 |
| records_object_20m | stringify | retain | 2 | 27046.292 → 27077.708 | +0.47% | True | +0.109 |
| records_object_8m | parse | discard | 20 | 20033.619 → 19708.396 | -1.30% | True | -0.141 |
| records_object_8m | parse | latest | 20 | 20032.808 → 19723.367 | -0.18% | True | -2.062 |
| records_object_8m | parse | retain | 2 | 19583.000 → 19203.416 | +16.79% | True | +2.531 |
| records_object_8m | roundtrip | discard | 20 | 31179.760 → 41575.260 | +16.96% | True | -90.281 |
| records_object_8m | stringify | discard | 20 | 12894.729 → 12887.240 | -0.05% | False | +0.109 |
| records_object_8m | stringify | latest | 20 | 12900.406 → 12888.096 | -0.10% | False | +0.094 |
| records_object_8m | stringify | retain | 2 | 10913.292 → 10882.521 | +0.05% | False | +0.109 |
| small_record | parse | discard | 100000 | 0.516 → 0.525 | +1.37% | False | +0.078 |
| small_record | parse | latest | 100000 | 0.518 → 0.523 | +0.88% | False | +0.094 |
| small_record | parse | retain | 100000 | 0.524 → 0.540 | +1.68% | True | +0.109 |
| small_record | roundtrip | discard | 100000 | 0.816 → 0.818 | +0.16% | False | +0.000 |
| small_record | stringify | discard | 100000 | 0.288 → 0.287 | -0.21% | False | +0.125 |
| small_record | stringify | latest | 100000 | 0.288 → 0.287 | -0.31% | False | +0.109 |
| small_record | stringify | retain | 100000 | 0.289 → 0.288 | -0.37% | False | +0.125 |
| wide_1m | parse | discard | 60 | 10490.874 → 10406.786 | -43.25% | True | -63.672 |
| wide_1m | parse | latest | 60 | 10621.540 → 10440.849 | -43.95% | True | -47.531 |
| wide_1m | parse | retain | 8 | 10458.641 → 10489.594 | +0.37% | True | +0.156 |
| wide_1m | roundtrip | discard | 60 | 13193.103 → 13201.956 | -17.83% | True | +20.969 |
| wide_1m | stringify | discard | 60 | 1650.572 → 1664.425 | +0.71% | True | +0.125 |
| wide_1m | stringify | latest | 60 | 1647.215 → 1688.863 | +2.62% | True | +0.125 |
| wide_1m | stringify | retain | 8 | 1546.708 → 1562.037 | +0.86% | True | +0.156 |

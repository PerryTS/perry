# Five repeated comparisons: all 38 CPU rows

CPU includes user and system time across the loop. Ranges are observed minima and maxima, not statistical confidence intervals. Parent is construction batch; checkpoint is bounded deferral.

| Fixture | Operation | Parent µs | Checkpoint µs | Candidate µs | vs checkpoint | Separated | Peak RSS change MiB |
|---|---|---:|---:|---:|---:|---|---:|
| empty_object | parse | 0.057 | 0.057 | 0.058 | +1.17% | True | +0.062 |
| empty_object | stringify | 0.047 | 0.047 | 0.047 | +0.20% | True | -0.031 |
| escaped_1m | parse | 2041.073 | 2043.524 | 2039.122 | -0.22% | False | +0.078 |
| escaped_1m | stringify | 951.220 | 952.839 | 952.274 | -0.06% | False | +0.062 |
| heterogeneous_1m | parse | 1931.412 | 1932.487 | 1985.825 | +2.76% | True | +0.062 |
| heterogeneous_1m | stringify | 848.227 | 847.564 | 851.242 | +0.43% | True | +0.109 |
| long_string_1m | parse | 203.297 | 202.893 | 203.645 | +0.37% | False | +0.078 |
| long_string_1m | stringify | 172.350 | 173.408 | 174.613 | +0.70% | False | +0.062 |
| null | parse | 0.014 | 0.014 | 0.014 | +0.05% | False | -0.047 |
| null | stringify | 0.007 | 0.007 | 0.007 | +0.40% | False | -0.047 |
| numbers_1m | parse | 1735.978 | 1735.946 | 1711.022 | -1.44% | True | +0.062 |
| numbers_1m | stringify | 1603.802 | 1604.875 | 1642.344 | +2.33% | True | +0.047 |
| object_1k | parse | 0.542 | 0.541 | 0.531 | -1.73% | True | +0.062 |
| object_1k | stringify | 0.237 | 0.237 | 0.236 | -0.45% | True | +0.062 |
| records_array_16k | parse | 23.817 | 23.829 | 24.453 | +2.62% | True | +0.062 |
| records_array_16k | stringify | 23.622 | 23.372 | 23.328 | -0.19% | False | +0.125 |
| records_array_1m | parse | 1552.600 | 1554.400 | 1594.895 | +2.61% | True | +0.062 |
| records_array_1m | stringify | 1497.915 | 1506.418 | 1501.243 | -0.34% | False | +0.125 |
| records_array_20m | parse | 53297.667 | 52927.000 | 53025.667 | +0.19% | False | +0.094 |
| records_array_20m | stringify | 81679.000 | 82124.875 | 82039.750 | -0.10% | False | +0.094 |
| records_array_8m | parse | 13374.600 | 13410.400 | 13728.500 | +2.37% | True | -0.469 |
| records_array_8m | stringify | 12955.350 | 13038.850 | 13011.700 | -0.21% | False | +0.094 |
| records_object_1m | parse | 2835.423 | 2820.493 | 2818.113 | -0.08% | False | +0.062 |
| records_object_1m | stringify | 1496.207 | 1510.874 | 1500.080 | -0.71% | True | +0.125 |
| records_object_20m | parse | 53297.667 | 52920.333 | 53001.333 | +0.15% | False | +0.094 |
| records_object_20m | stringify | 82042.375 | 82147.000 | 82015.250 | -0.16% | False | +0.062 |
| records_object_8m | parse | 21645.000 | 21452.250 | 21467.500 | +0.07% | False | +0.094 |
| records_object_8m | stringify | 12887.190 | 12954.476 | 12913.381 | -0.32% | False | +0.078 |
| small_record | parse | 0.542 | 0.541 | 0.532 | -1.60% | True | +0.062 |
| small_record | stringify | 0.284 | 0.284 | 0.285 | +0.17% | False | +0.062 |
| string_a | parse | 0.016 | 0.016 | 0.017 | +1.94% | True | -0.062 |
| string_a | stringify | 0.010 | 0.010 | 0.010 | +0.09% | False | -0.047 |
| tiny_object | parse | 0.122 | 0.121 | 0.122 | +0.51% | True | +0.062 |
| tiny_object | stringify | 0.093 | 0.093 | 0.092 | -0.89% | True | +0.078 |
| unicode_1m | parse | 258.006 | 256.993 | 258.030 | +0.40% | False | +0.062 |
| unicode_1m | stringify | 139.361 | 138.386 | 138.783 | +0.29% | False | +0.047 |
| wide_1m | parse | 20386.861 | 15215.056 | 15340.000 | +0.82% | True | -0.969 |
| wide_1m | stringify | 1690.986 | 1710.084 | 1710.116 | +0.00% | False | +0.062 |

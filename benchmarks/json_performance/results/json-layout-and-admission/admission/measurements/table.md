# Five repeated comparisons: all 38 CPU rows

CPU includes user and system time across the loop. Ranges are observed minima and maxima, not statistical confidence intervals. Parent is construction batch; checkpoint is bounded deferral.

| Fixture | Operation | Parent µs | Checkpoint µs | Candidate µs | vs checkpoint | Separated | Peak RSS change MiB |
|---|---|---:|---:|---:|---:|---|---:|
| empty_object | parse | 0.057 | 0.057 | 0.057 | +0.34% | True | +0.109 |
| empty_object | stringify | 0.047 | 0.047 | 0.047 | -0.64% | True | +0.062 |
| escaped_1m | parse | 2041.537 | 2044.000 | 2038.561 | -0.27% | False | +0.125 |
| escaped_1m | stringify | 950.994 | 951.000 | 963.958 | +1.36% | True | +0.125 |
| heterogeneous_1m | parse | 1932.125 | 1935.062 | 1933.487 | -0.08% | False | +0.078 |
| heterogeneous_1m | stringify | 848.393 | 846.886 | 852.289 | +0.64% | False | +0.109 |
| long_string_1m | parse | 203.554 | 202.645 | 204.026 | +0.68% | False | +0.078 |
| long_string_1m | stringify | 172.581 | 173.514 | 173.896 | +0.22% | False | +0.062 |
| null | parse | 0.014 | 0.014 | 0.014 | -0.08% | False | -0.031 |
| null | stringify | 0.007 | 0.007 | 0.007 | +0.05% | False | +0.016 |
| numbers_1m | parse | 1737.750 | 1710.598 | 1710.228 | -0.02% | False | +0.078 |
| numbers_1m | stringify | 1605.312 | 1610.938 | 1603.938 | -0.43% | False | +0.297 |
| object_1k | parse | 0.542 | 0.542 | 0.539 | -0.48% | False | +0.109 |
| object_1k | stringify | 0.237 | 0.237 | 0.237 | +0.01% | False | +0.125 |
| records_array_16k | parse | 23.809 | 23.816 | 23.823 | +0.03% | False | +0.078 |
| records_array_16k | stringify | 23.564 | 23.367 | 23.312 | -0.24% | False | +0.203 |
| records_array_1m | parse | 1553.842 | 1551.432 | 1552.937 | +0.10% | False | +0.094 |
| records_array_1m | stringify | 1495.215 | 1508.062 | 1495.102 | -0.86% | True | +0.156 |
| records_array_20m | parse | 53383.000 | 53110.667 | 52959.000 | -0.29% | False | +0.125 |
| records_array_20m | stringify | 81687.125 | 81946.875 | 81727.000 | -0.27% | True | +0.203 |
| records_array_8m | parse | 13390.400 | 13408.100 | 13397.800 | -0.08% | False | +0.109 |
| records_array_8m | stringify | 12980.500 | 13044.500 | 12968.200 | -0.58% | True | +0.094 |
| records_object_1m | parse | 2833.113 | 2820.155 | 2818.127 | -0.07% | False | +0.141 |
| records_object_1m | stringify | 1501.264 | 1510.534 | 1490.989 | -1.29% | True | +0.125 |
| records_object_20m | parse | 53278.333 | 52994.000 | 53027.000 | +0.06% | False | +0.125 |
| records_object_20m | stringify | 81884.375 | 82036.000 | 81767.125 | -0.33% | False | +0.172 |
| records_object_8m | parse | 21628.500 | 21445.625 | 21457.250 | +0.05% | False | +0.172 |
| records_object_8m | stringify | 12887.810 | 12964.381 | 12880.333 | -0.65% | True | +0.062 |
| small_record | parse | 0.542 | 0.540 | 0.539 | -0.12% | False | +0.109 |
| small_record | stringify | 0.284 | 0.284 | 0.278 | -2.26% | True | +0.109 |
| string_a | parse | 0.016 | 0.016 | 0.016 | -0.02% | False | -0.016 |
| string_a | stringify | 0.010 | 0.010 | 0.010 | +0.10% | False | +0.031 |
| tiny_object | parse | 0.122 | 0.121 | 0.122 | +0.31% | False | +0.125 |
| tiny_object | stringify | 0.093 | 0.093 | 0.094 | +0.76% | True | +0.141 |
| unicode_1m | parse | 256.983 | 257.143 | 256.405 | -0.29% | False | +0.062 |
| unicode_1m | stringify | 137.504 | 137.257 | 138.924 | +1.21% | False | +0.047 |
| wide_1m | parse | 20401.361 | 15193.667 | 15190.639 | -0.02% | False | -2.078 |
| wide_1m | stringify | 1690.130 | 1709.084 | 1691.823 | -1.01% | True | +0.078 |

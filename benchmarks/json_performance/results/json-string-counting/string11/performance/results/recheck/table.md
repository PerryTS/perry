# Five repeated comparisons: all 38 CPU rows

CPU includes user and system time across the loop. Ranges are observed minima and maxima, not statistical confidence intervals. Both Perry arms use default release settings. Candidate counts UTF-16 units in one pass on ARM, preserving the bounded scalar interpretation.

| Fixture | Operation | Checkpoint µs | Candidate µs | vs checkpoint | Separated | Peak RSS change MiB |
|---|---|---:|---:|---:|---|---:|
| empty_object | parse | 0.056 | 0.056 | +0.37% | True | -0.016 |
| empty_object | stringify | 0.048 | 0.048 | -0.06% | False | -0.062 |
| escaped_1m | parse | 2451.488 | 2454.768 | +0.13% | False | -0.016 |
| escaped_1m | stringify | 962.149 | 962.667 | +0.05% | False | -0.016 |
| heterogeneous_1m | parse | 1853.925 | 1855.700 | +0.10% | False | -0.016 |
| heterogeneous_1m | stringify | 795.071 | 795.450 | +0.05% | False | -0.016 |
| long_string_1m | parse | 201.636 | 200.789 | -0.42% | False | -0.016 |
| long_string_1m | stringify | 171.513 | 170.444 | -0.62% | False | -0.031 |
| null | parse | 0.012 | 0.012 | -0.02% | False | -0.031 |
| null | stringify | 0.007 | 0.007 | -0.01% | False | -0.047 |
| numbers_1m | parse | 1703.283 | 1704.837 | +0.09% | False | -0.016 |
| numbers_1m | stringify | 1541.208 | 1551.125 | +0.64% | False | -0.016 |
| object_1k | parse | 0.519 | 0.517 | -0.29% | True | +0.031 |
| object_1k | stringify | 0.238 | 0.237 | -0.21% | False | -0.016 |
| records_array_16k | parse | 22.505 | 22.496 | -0.04% | False | -0.016 |
| records_array_16k | stringify | 22.967 | 22.740 | -0.99% | True | -0.031 |
| records_array_1m | parse | 1473.084 | 1470.884 | -0.15% | False | -0.031 |
| records_array_1m | stringify | 1458.593 | 1457.023 | -0.11% | False | -0.047 |
| records_array_20m | parse | 54715.333 | 54681.333 | -0.06% | False | -0.031 |
| records_array_20m | stringify | 78136.625 | 77994.500 | -0.18% | False | -0.062 |
| records_array_8m | parse | 12752.300 | 12768.800 | +0.13% | False | -0.047 |
| records_array_8m | stringify | 12632.250 | 12661.700 | +0.23% | False | +0.031 |
| records_object_1m | parse | 2887.451 | 2886.761 | -0.02% | False | -0.047 |
| records_object_1m | stringify | 1452.994 | 1452.316 | -0.05% | False | -0.047 |
| records_object_20m | parse | 54640.667 | 54604.000 | -0.07% | False | -0.031 |
| records_object_20m | stringify | 78176.000 | 78134.250 | -0.05% | False | -0.062 |
| records_object_8m | parse | 22156.875 | 22170.500 | +0.06% | False | -0.078 |
| records_object_8m | stringify | 12548.048 | 12544.048 | -0.03% | False | +0.031 |
| small_record | parse | 0.523 | 0.523 | -0.03% | False | -0.016 |
| small_record | stringify | 0.285 | 0.285 | +0.26% | True | +0.000 |
| string_a | parse | 0.015 | 0.015 | +0.05% | False | -0.062 |
| string_a | stringify | 0.010 | 0.010 | +0.04% | False | -0.047 |
| tiny_object | parse | 0.123 | 0.123 | +0.03% | False | -0.016 |
| tiny_object | stringify | 0.094 | 0.094 | -0.69% | False | -0.016 |
| unicode_1m | parse | 254.420 | 221.118 | -13.09% | True | -0.047 |
| unicode_1m | stringify | 135.923 | 137.210 | +0.95% | False | -0.047 |
| wide_1m | parse | 15107.500 | 15120.000 | +0.08% | False | +0.000 |
| wide_1m | stringify | 1671.651 | 1671.372 | -0.02% | False | -0.016 |

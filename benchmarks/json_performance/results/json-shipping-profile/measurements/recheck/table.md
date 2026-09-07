# Five repeated comparisons: all 38 CPU rows

CPU includes user and system time across the loop. Ranges are observed minima and maxima, not statistical confidence intervals. Parent is the retained source with 16 runtime/stdlib codegen units; checkpoint is the same source with default release settings; candidate uses default release settings and the array-prefix patch.

| Fixture | Operation | Parent µs | Checkpoint µs | Candidate µs | vs checkpoint | Separated | Peak RSS change MiB |
|---|---|---:|---:|---:|---:|---|---:|
| empty_object | parse | 0.057 | 0.056 | 0.056 | -0.10% | False | -0.016 |
| empty_object | stringify | 0.047 | 0.048 | 0.048 | +0.14% | True | -0.016 |
| escaped_1m | parse | 2038.244 | 2451.024 | 2452.366 | +0.05% | False | -0.016 |
| escaped_1m | stringify | 950.732 | 961.851 | 963.411 | +0.16% | False | +0.000 |
| heterogeneous_1m | parse | 1933.700 | 1854.075 | 1854.287 | +0.01% | False | +0.000 |
| heterogeneous_1m | stringify | 845.754 | 794.957 | 789.972 | -0.63% | False | -0.016 |
| long_string_1m | parse | 202.687 | 200.631 | 201.677 | +0.52% | False | -0.016 |
| long_string_1m | stringify | 173.598 | 172.250 | 172.572 | +0.19% | False | +0.000 |
| null | parse | 0.014 | 0.012 | 0.012 | +0.00% | False | -0.016 |
| null | stringify | 0.007 | 0.007 | 0.007 | -0.27% | False | -0.016 |
| numbers_1m | parse | 1709.685 | 1701.815 | 1704.478 | +0.16% | False | -0.016 |
| numbers_1m | stringify | 1604.927 | 1545.917 | 1543.062 | -0.18% | False | +0.000 |
| object_1k | parse | 0.540 | 0.519 | 0.519 | +0.03% | False | -0.031 |
| object_1k | stringify | 0.237 | 0.238 | 0.238 | -0.09% | False | -0.016 |
| records_array_16k | parse | 23.821 | 22.501 | 22.498 | -0.01% | False | -0.016 |
| records_array_16k | stringify | 23.418 | 22.954 | 22.969 | +0.06% | False | +0.000 |
| records_array_1m | parse | 1553.811 | 1475.789 | 1473.684 | -0.14% | False | +0.000 |
| records_array_1m | stringify | 1509.085 | 1456.949 | 1458.232 | +0.09% | False | -0.016 |
| records_array_20m | parse | 52951.667 | 54622.000 | 54735.333 | +0.21% | False | -0.328 |
| records_array_20m | stringify | 82025.625 | 78050.875 | 78013.375 | -0.05% | False | +1.328 |
| records_array_8m | parse | 13410.000 | 12773.300 | 12785.900 | +0.10% | False | +0.000 |
| records_array_8m | stringify | 13039.600 | 12622.600 | 12632.050 | +0.07% | False | -0.016 |
| records_object_1m | parse | 2821.930 | 2889.056 | 2902.817 | +0.48% | True | -0.016 |
| records_object_1m | stringify | 1509.971 | 1451.603 | 1452.063 | +0.03% | False | -0.016 |
| records_object_20m | parse | 52938.000 | 54650.000 | 54881.000 | +0.42% | False | -0.328 |
| records_object_20m | stringify | 82082.625 | 78256.000 | 78180.000 | -0.10% | False | +1.328 |
| records_object_8m | parse | 21470.500 | 22139.000 | 22236.250 | +0.44% | False | -0.062 |
| records_object_8m | stringify | 12943.857 | 12567.952 | 12540.238 | -0.22% | False | -0.016 |
| small_record | parse | 0.539 | 0.524 | 0.524 | +0.08% | False | -0.016 |
| small_record | stringify | 0.284 | 0.284 | 0.284 | -0.24% | False | -0.016 |
| string_a | parse | 0.016 | 0.015 | 0.015 | -0.06% | False | -0.016 |
| string_a | stringify | 0.010 | 0.010 | 0.010 | +0.04% | False | -0.016 |
| tiny_object | parse | 0.121 | 0.123 | 0.123 | +0.03% | False | -0.016 |
| tiny_object | stringify | 0.093 | 0.094 | 0.094 | +0.01% | False | -0.016 |
| unicode_1m | parse | 256.505 | 255.672 | 255.557 | -0.04% | False | -0.016 |
| unicode_1m | stringify | 136.793 | 135.256 | 136.031 | +0.57% | False | +0.000 |
| wide_1m | parse | 15200.972 | 15137.778 | 15126.389 | -0.08% | False | +0.969 |
| wide_1m | stringify | 1709.730 | 1670.702 | 1670.628 | -0.00% | False | -0.016 |

# Five repeated comparisons: all 38 CPU rows

CPU includes user and system time across the loop. Ranges are observed minima and maxima, not statistical confidence intervals. Parent is construction batch; checkpoint is bounded deferral.

| Fixture | Operation | Parent µs | Checkpoint µs | Candidate µs | vs checkpoint | Separated | Peak RSS change MiB |
|---|---|---:|---:|---:|---:|---|---:|
| empty_object | parse | 0.057 | 0.057 | 0.059 | +3.06% | True | +0.016 |
| empty_object | stringify | 0.047 | 0.047 | 0.048 | +0.98% | True | +0.000 |
| escaped_1m | parse | 2040.671 | 2041.024 | 2041.524 | +0.02% | False | +0.047 |
| escaped_1m | stringify | 951.976 | 951.935 | 966.310 | +1.51% | True | +0.062 |
| heterogeneous_1m | parse | 1933.275 | 1935.213 | 1932.088 | -0.16% | False | +0.062 |
| heterogeneous_1m | stringify | 847.076 | 847.005 | 849.436 | +0.29% | False | +0.109 |
| long_string_1m | parse | 203.327 | 203.683 | 202.864 | -0.40% | False | +0.047 |
| long_string_1m | stringify | 175.828 | 174.490 | 173.797 | -0.40% | False | +0.031 |
| null | parse | 0.014 | 0.014 | 0.015 | +9.07% | True | -0.031 |
| null | stringify | 0.007 | 0.007 | 0.006 | -4.74% | True | -0.031 |
| numbers_1m | parse | 1737.674 | 1736.250 | 1739.000 | +0.16% | False | +0.062 |
| numbers_1m | stringify | 1604.594 | 1605.615 | 1607.740 | +0.13% | False | +0.062 |
| object_1k | parse | 0.542 | 0.541 | 0.540 | -0.13% | False | +0.000 |
| object_1k | stringify | 0.237 | 0.237 | 0.238 | +0.26% | False | +0.031 |
| records_array_16k | parse | 23.833 | 23.796 | 23.811 | +0.06% | False | +0.062 |
| records_array_16k | stringify | 23.529 | 23.394 | 23.297 | -0.42% | True | +0.109 |
| records_array_1m | parse | 1551.432 | 1553.916 | 1553.568 | -0.02% | False | +0.062 |
| records_array_1m | stringify | 1493.277 | 1510.113 | 1490.712 | -1.28% | False | +0.141 |
| records_array_20m | parse | 53282.333 | 52905.333 | 53278.000 | +0.70% | True | +0.000 |
| records_array_20m | stringify | 81769.500 | 82054.500 | 81886.875 | -0.20% | False | +0.109 |
| records_array_8m | parse | 13396.200 | 13387.900 | 13404.700 | +0.13% | False | +0.078 |
| records_array_8m | stringify | 12956.300 | 13050.400 | 12975.750 | -0.57% | True | +0.125 |
| records_object_1m | parse | 2834.042 | 2823.338 | 2834.746 | +0.40% | False | +0.016 |
| records_object_1m | stringify | 1499.621 | 1510.994 | 1490.747 | -1.34% | True | +0.109 |
| records_object_20m | parse | 53345.333 | 52983.333 | 53355.000 | +0.70% | True | +0.000 |
| records_object_20m | stringify | 81943.000 | 82289.250 | 81931.500 | -0.43% | True | +0.094 |
| records_object_8m | parse | 21626.625 | 21486.250 | 21576.500 | +0.42% | True | +0.000 |
| records_object_8m | stringify | 12887.190 | 12958.952 | 12867.095 | -0.71% | True | +0.094 |
| small_record | parse | 0.543 | 0.540 | 0.544 | +0.65% | True | +0.016 |
| small_record | stringify | 0.284 | 0.284 | 0.279 | -1.73% | True | +0.062 |
| string_a | parse | 0.016 | 0.016 | 0.018 | +7.72% | True | -0.031 |
| string_a | stringify | 0.010 | 0.010 | 0.010 | +0.02% | False | -0.016 |
| tiny_object | parse | 0.122 | 0.122 | 0.122 | +0.27% | True | +0.016 |
| tiny_object | stringify | 0.093 | 0.093 | 0.095 | +1.62% | True | +0.047 |
| unicode_1m | parse | 257.967 | 258.270 | 257.028 | -0.48% | False | +0.031 |
| unicode_1m | stringify | 140.512 | 138.384 | 138.256 | -0.09% | False | +0.031 |
| wide_1m | parse | 20361.417 | 15218.972 | 15234.167 | +0.10% | False | +1.188 |
| wide_1m | stringify | 1692.781 | 1709.595 | 1691.093 | -1.08% | True | +0.047 |

# Five repeated comparisons: all 38 CPU rows

CPU includes user and system time across the loop. Ranges are observed minima and maxima, not statistical confidence intervals. Parent is construction batch; checkpoint is bounded deferral.

| Fixture | Operation | Parent µs | Checkpoint µs | Candidate µs | vs checkpoint | Separated | Peak RSS change MiB |
|---|---|---:|---:|---:|---:|---|---:|
| empty_object | parse | 0.057 | 0.057 | 0.057 | -0.01% | False | +0.000 |
| empty_object | stringify | 0.047 | 0.047 | 0.047 | +0.03% | False | +0.016 |
| escaped_1m | parse | 2040.854 | 2039.049 | 2040.451 | +0.07% | False | +0.047 |
| escaped_1m | stringify | 951.095 | 952.137 | 951.744 | -0.04% | False | +0.047 |
| heterogeneous_1m | parse | 1931.737 | 1934.062 | 1934.600 | +0.03% | False | +0.000 |
| heterogeneous_1m | stringify | 847.199 | 848.564 | 839.773 | -1.04% | False | +0.000 |
| long_string_1m | parse | 204.605 | 202.560 | 204.041 | +0.73% | False | +0.000 |
| long_string_1m | stringify | 172.641 | 172.505 | 173.829 | +0.77% | False | +0.000 |
| null | parse | 0.014 | 0.014 | 0.014 | -0.01% | False | +0.000 |
| null | stringify | 0.007 | 0.007 | 0.007 | -0.07% | False | -0.016 |
| numbers_1m | parse | 1732.783 | 1710.424 | 1735.163 | +1.45% | False | +0.000 |
| numbers_1m | stringify | 1605.885 | 1606.104 | 1605.802 | -0.02% | False | +0.219 |
| object_1k | parse | 0.542 | 0.540 | 0.541 | +0.10% | False | +0.016 |
| object_1k | stringify | 0.237 | 0.237 | 0.237 | -0.06% | False | -0.016 |
| records_array_16k | parse | 23.839 | 23.841 | 23.847 | +0.02% | False | +0.000 |
| records_array_16k | stringify | 23.588 | 23.398 | 23.435 | +0.16% | False | +0.016 |
| records_array_1m | parse | 1553.768 | 1556.821 | 1555.768 | -0.07% | False | +0.016 |
| records_array_1m | stringify | 1495.480 | 1506.525 | 1507.876 | +0.09% | False | +0.031 |
| records_array_20m | parse | 53353.000 | 52919.000 | 52902.333 | -0.03% | False | -0.297 |
| records_array_20m | stringify | 81708.625 | 81894.625 | 81747.250 | -0.18% | False | +1.391 |
| records_array_8m | parse | 13407.200 | 13394.500 | 13414.700 | +0.15% | False | +0.047 |
| records_array_8m | stringify | 12973.000 | 13051.900 | 13055.150 | +0.02% | False | -0.047 |
| records_object_1m | parse | 2835.789 | 2820.803 | 2824.197 | +0.12% | False | +0.031 |
| records_object_1m | stringify | 1499.052 | 1511.454 | 1510.282 | -0.08% | False | +0.031 |
| records_object_20m | parse | 53329.333 | 52947.667 | 53035.000 | +0.16% | False | -0.297 |
| records_object_20m | stringify | 81860.500 | 82049.500 | 81962.375 | -0.11% | False | +1.391 |
| records_object_8m | parse | 21611.875 | 21548.250 | 21513.000 | -0.16% | False | +0.016 |
| records_object_8m | stringify | 12888.571 | 12942.286 | 12922.286 | -0.15% | False | -0.047 |
| small_record | parse | 0.543 | 0.540 | 0.542 | +0.36% | False | +0.000 |
| small_record | stringify | 0.286 | 0.284 | 0.284 | -0.00% | False | +0.000 |
| string_a | parse | 0.016 | 0.016 | 0.016 | -0.01% | False | +0.016 |
| string_a | stringify | 0.010 | 0.010 | 0.010 | +0.03% | False | +0.016 |
| tiny_object | parse | 0.122 | 0.121 | 0.121 | -0.07% | False | +0.000 |
| tiny_object | stringify | 0.093 | 0.093 | 0.093 | +0.01% | False | +0.000 |
| unicode_1m | parse | 258.172 | 256.220 | 257.005 | +0.31% | False | +0.000 |
| unicode_1m | stringify | 138.174 | 137.225 | 137.663 | +0.32% | False | +0.000 |
| wide_1m | parse | 21007.972 | 15207.417 | 15259.889 | +0.35% | False | -0.438 |
| wide_1m | stringify | 1693.498 | 1707.865 | 1707.158 | -0.04% | False | -0.047 |

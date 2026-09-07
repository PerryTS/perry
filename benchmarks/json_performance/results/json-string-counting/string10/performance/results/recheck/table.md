# Five repeated comparisons: all 38 CPU rows

CPU includes user and system time across the loop. Ranges are observed minima and maxima, not statistical confidence intervals. Both Perry arms use default release settings. Candidate uses the basic UTF-8 validator in UTF-16 counting.

| Fixture | Operation | Checkpoint µs | Candidate µs | vs checkpoint | Separated | Peak RSS change MiB |
|---|---|---:|---:|---:|---|---:|
| empty_object | parse | 0.056 | 0.056 | +0.23% | False | +0.047 |
| empty_object | stringify | 0.048 | 0.048 | -0.07% | False | +0.031 |
| escaped_1m | parse | 2452.280 | 2455.305 | +0.12% | False | +0.031 |
| escaped_1m | stringify | 962.857 | 963.887 | +0.11% | False | +0.016 |
| heterogeneous_1m | parse | 1853.013 | 1854.150 | +0.06% | False | +0.031 |
| heterogeneous_1m | stringify | 793.872 | 794.991 | +0.14% | False | +0.031 |
| long_string_1m | parse | 200.126 | 202.644 | +1.26% | False | +0.047 |
| long_string_1m | stringify | 170.737 | 171.102 | +0.21% | False | +0.031 |
| null | parse | 0.012 | 0.012 | -0.09% | False | +0.031 |
| null | stringify | 0.007 | 0.007 | +0.08% | False | +0.031 |
| numbers_1m | parse | 1728.717 | 1705.446 | -1.35% | False | +0.031 |
| numbers_1m | stringify | 1543.312 | 1548.635 | +0.34% | False | +0.047 |
| object_1k | parse | 0.519 | 0.518 | -0.30% | False | +0.031 |
| object_1k | stringify | 0.239 | 0.238 | -0.49% | True | +0.016 |
| records_array_16k | parse | 22.485 | 22.509 | +0.11% | False | +0.031 |
| records_array_16k | stringify | 22.994 | 22.999 | +0.02% | False | +0.031 |
| records_array_1m | parse | 1473.811 | 1471.453 | -0.16% | False | +0.031 |
| records_array_1m | stringify | 1457.503 | 1459.068 | +0.11% | False | +0.031 |
| records_array_20m | parse | 54620.667 | 54641.000 | +0.04% | False | +0.031 |
| records_array_20m | stringify | 78203.500 | 78073.125 | -0.17% | False | +0.031 |
| records_array_8m | parse | 12782.600 | 12755.100 | -0.22% | False | +0.031 |
| records_array_8m | stringify | 12612.550 | 12614.100 | +0.01% | False | +0.031 |
| records_object_1m | parse | 2889.859 | 2891.352 | +0.05% | False | +0.031 |
| records_object_1m | stringify | 1454.569 | 1455.856 | +0.09% | False | +0.031 |
| records_object_20m | parse | 54757.667 | 54683.667 | -0.14% | False | +0.031 |
| records_object_20m | stringify | 78152.625 | 78195.125 | +0.05% | False | +0.016 |
| records_object_8m | parse | 22185.125 | 22181.625 | -0.02% | False | +0.031 |
| records_object_8m | stringify | 12541.762 | 12560.762 | +0.15% | False | +0.031 |
| small_record | parse | 0.523 | 0.523 | +0.01% | False | +0.031 |
| small_record | stringify | 0.285 | 0.284 | -0.25% | False | +0.031 |
| string_a | parse | 0.015 | 0.015 | +0.05% | False | +0.031 |
| string_a | stringify | 0.010 | 0.010 | -0.04% | False | +0.031 |
| tiny_object | parse | 0.124 | 0.124 | -0.05% | False | +0.031 |
| tiny_object | stringify | 0.094 | 0.094 | -0.31% | False | +0.031 |
| unicode_1m | parse | 255.599 | 253.090 | -0.98% | False | +0.031 |
| unicode_1m | stringify | 136.110 | 137.276 | +0.86% | False | +0.031 |
| wide_1m | parse | 15124.889 | 15105.083 | -0.13% | False | -0.953 |
| wide_1m | stringify | 1670.893 | 1670.888 | -0.00% | False | +0.031 |

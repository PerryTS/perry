Source bf779ee97 (test-only fixture follow-up 9a0a32b5f); median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.013 | 0.027 | 0.019 | 0.681× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.692× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.337× |
| empty_object | parse | 0.057 | 0.045 | 0.024 | 2.348× |
| empty_object | stringify | 0.050 | 0.032 | 0.030 | 1.664× |
| tiny_object | parse | 0.354 | 0.082 | 0.045 | 7.831× |
| tiny_object | stringify | 0.095 | 0.037 | 0.040 | 2.554× |
| small_record | parse | 0.673 | 0.342 | 0.252 | 2.674× |
| small_record | stringify | 0.324 | 0.108 | 0.121 | 2.990× |
| object_1k | parse | 0.590 | 0.544 | 0.239 | 2.467× |
| object_1k | stringify | 0.245 | 0.199 | 0.215 | 1.230× |
| records_array_16k | parse | 23.805 | 39.603 | 33.941 | 0.701× |
| records_array_16k | stringify | 24.307 | 13.246 | 23.266 | 1.835× |
| records_array_1m | parse | 1,551.458 | 2,818.000 | 2,137.219 | 0.726× |
| records_array_1m | stringify | 1,548.034 | 840.307 | 971.142 | 1.842× |
| records_object_1m | parse | 4,006.901 | 2,757.521 | 2,134.338 | 1.877× |
| records_object_1m | stringify | 1,555.466 | 847.747 | 969.489 | 1.835× |
| records_array_8m | parse | 13,455.900 | 34,530.200 | 20,864.100 | 0.645× |
| records_array_8m | stringify | 13,477.200 | 6,799.850 | 8,405.150 | 1.982× |
| records_object_8m | parse | 31,399.625 | 30,725.250 | 21,816.125 | 1.439× |
| records_object_8m | stringify | 13,469.900 | 6,819.550 | 8,497.550 | 1.975× |
| records_array_20m | parse | 77,451.000 | 93,416.000 | 56,702.000 | 1.366× |
| records_array_20m | stringify | 85,911.875 | 17,348.875 | 20,817.750 | 4.952× |
| records_object_20m | parse | 77,803.333 | 97,328.333 | 56,606.333 | 1.374× |
| records_object_20m | stringify | 86,043.375 | 17,323.500 | 20,918.250 | 4.967× |
| numbers_1m | parse | 1,713.967 | 3,126.978 | 3,208.196 | 0.548× |
| numbers_1m | stringify | 1,605.802 | 1,980.521 | 2,953.750 | 0.811× |
| long_string_1m | parse | 204.806 | 368.810 | 67.656 | 3.027× |
| long_string_1m | stringify | 175.341 | 105.373 | 97.298 | 1.802× |
| escaped_1m | parse | 2,040.317 | 1,733.927 | 2,087.537 | 1.177× |
| escaped_1m | stringify | 1,569.730 | 1,904.310 | 2,091.690 | 0.824× |
| unicode_1m | parse | 256.259 | 438.206 | 60.439 | 4.240× |
| unicode_1m | stringify | 139.551 | 410.969 | 449.966 | 0.340× |
| wide_1m | parse | 20,897.139 | 4,930.528 | 4,159.333 | 5.024× |
| wide_1m | stringify | 1,690.281 | 6,359.627 | 666.244 | 2.537× |
| heterogeneous_1m | parse | 1,932.750 | 3,930.525 | 2,940.387 | 0.657× |
| heterogeneous_1m | stringify | 4,424.461 | 905.539 | 1,061.174 | 4.886× |

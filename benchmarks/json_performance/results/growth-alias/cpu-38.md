Source 452c329aa481bb2d7bafb0955adcf1193e254864; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | 0.722× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.722× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.338× |
| empty_object | parse | 0.057 | 0.045 | 0.024 | 2.367× |
| empty_object | stringify | 0.047 | 0.032 | 0.030 | 1.574× |
| tiny_object | parse | 0.120 | 0.082 | 0.045 | 2.656× |
| tiny_object | stringify | 0.091 | 0.037 | 0.040 | 2.443× |
| small_record | parse | 0.677 | 0.355 | 0.252 | 2.689× |
| small_record | stringify | 0.281 | 0.108 | 0.120 | 2.587× |
| object_1k | parse | 0.596 | 0.541 | 0.237 | 2.518× |
| object_1k | stringify | 0.233 | 0.198 | 0.216 | 1.175× |
| records_array_16k | parse | 23.828 | 40.881 | 33.941 | 0.702× |
| records_array_16k | stringify | 23.224 | 13.328 | 23.208 | 1.742× |
| records_array_1m | parse | 1,550.895 | 2,881.316 | 2,135.379 | 0.726× |
| records_array_1m | stringify | 1,494.549 | 843.377 | 968.549 | 1.772× |
| records_object_1m | parse | 3,925.972 | 2,724.085 | 2,180.282 | 1.801× |
| records_object_1m | stringify | 1,494.972 | 848.165 | 970.773 | 1.763× |
| records_array_8m | parse | 13,383.300 | 33,994.400 | 20,924.800 | 0.640× |
| records_array_8m | stringify | 13,043.400 | 6,847.500 | 8,484.750 | 1.905× |
| records_object_8m | parse | 30,570.875 | 29,502.125 | 21,805.375 | 1.402× |
| records_object_8m | stringify | 12,963.850 | 6,840.000 | 8,481.100 | 1.895× |
| records_array_20m | parse | 75,454.333 | 95,347.333 | 56,494.667 | 1.336× |
| records_array_20m | stringify | 80,509.375 | 17,298.875 | 20,864.500 | 4.654× |
| records_object_20m | parse | 75,561.000 | 97,318.000 | 56,692.667 | 1.333× |
| records_object_20m | stringify | 80,293.375 | 17,343.250 | 20,835.000 | 4.630× |
| numbers_1m | parse | 1,711.630 | 3,124.902 | 3,181.804 | 0.548× |
| numbers_1m | stringify | 1,603.729 | 1,975.906 | 2,951.531 | 0.812× |
| long_string_1m | parse | 204.603 | 369.748 | 68.080 | 3.005× |
| long_string_1m | stringify | 173.204 | 105.673 | 96.034 | 1.804× |
| escaped_1m | parse | 2,042.256 | 1,733.256 | 2,082.427 | 1.178× |
| escaped_1m | stringify | 950.604 | 1,889.533 | 2,083.704 | 0.503× |
| unicode_1m | parse | 257.378 | 438.060 | 60.636 | 4.245× |
| unicode_1m | stringify | 138.415 | 413.657 | 452.819 | 0.335× |
| wide_1m | parse | 20,967.639 | 4,922.583 | 4,158.139 | 5.043× |
| wide_1m | stringify | 1,706.218 | 6,370.269 | 667.671 | 2.555× |
| heterogeneous_1m | parse | 1,932.899 | 3,857.038 | 2,966.228 | 0.652× |
| heterogeneous_1m | stringify | 4,348.745 | 907.752 | 1,060.224 | 4.791× |

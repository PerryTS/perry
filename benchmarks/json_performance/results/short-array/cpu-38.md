Source 2e79d122ba72e879a21ae6db65962339dc79c68d; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | 0.717× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.717× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.338× |
| empty_object | parse | 0.057 | 0.045 | 0.024 | 2.364× |
| empty_object | stringify | 0.047 | 0.032 | 0.030 | 1.575× |
| tiny_object | parse | 0.120 | 0.081 | 0.045 | 2.652× |
| tiny_object | stringify | 0.091 | 0.037 | 0.040 | 2.448× |
| small_record | parse | 0.674 | 0.339 | 0.253 | 2.663× |
| small_record | stringify | 0.284 | 0.109 | 0.121 | 2.615× |
| object_1k | parse | 0.589 | 0.540 | 0.237 | 2.487× |
| object_1k | stringify | 0.235 | 0.199 | 0.215 | 1.182× |
| records_array_16k | parse | 23.789 | 39.136 | 33.964 | 0.700× |
| records_array_16k | stringify | 23.262 | 13.277 | 23.213 | 1.752× |
| records_array_1m | parse | 1,554.579 | 2,882.979 | 2,142.442 | 0.726× |
| records_array_1m | stringify | 1,490.271 | 849.571 | 969.198 | 1.754× |
| records_object_1m | parse | 3,988.676 | 2,851.873 | 2,137.197 | 1.866× |
| records_object_1m | stringify | 1,497.960 | 844.415 | 972.420 | 1.774× |
| records_array_8m | parse | 13,366.900 | 34,031.000 | 20,845.900 | 0.641× |
| records_array_8m | stringify | 13,006.350 | 6,813.900 | 8,507.850 | 1.909× |
| records_object_8m | parse | 30,940.125 | 29,214.000 | 21,769.250 | 1.421× |
| records_object_8m | stringify | 12,972.100 | 6,814.750 | 8,435.000 | 1.904× |
| records_array_20m | parse | 76,127.000 | 94,273.000 | 56,713.667 | 1.342× |
| records_array_20m | stringify | 80,218.500 | 17,383.250 | 21,036.750 | 4.615× |
| records_object_20m | parse | 76,178.333 | 93,859.667 | 56,862.667 | 1.340× |
| records_object_20m | stringify | 80,409.375 | 17,397.750 | 21,074.125 | 4.622× |
| numbers_1m | parse | 1,713.571 | 3,121.374 | 3,205.374 | 0.549× |
| numbers_1m | stringify | 1,609.573 | 1,980.417 | 2,952.969 | 0.813× |
| long_string_1m | parse | 203.833 | 368.833 | 67.705 | 3.011× |
| long_string_1m | stringify | 173.281 | 109.082 | 97.828 | 1.771× |
| escaped_1m | parse | 2,040.939 | 1,733.305 | 2,083.793 | 1.177× |
| escaped_1m | stringify | 952.833 | 1,894.661 | 2,082.530 | 0.503× |
| unicode_1m | parse | 257.056 | 438.080 | 59.267 | 4.337× |
| unicode_1m | stringify | 137.989 | 412.653 | 451.524 | 0.334× |
| wide_1m | parse | 20,897.500 | 4,935.028 | 4,173.750 | 5.007× |
| wide_1m | stringify | 1,690.556 | 6,355.903 | 665.755 | 2.539× |
| heterogeneous_1m | parse | 1,933.063 | 3,947.013 | 2,972.456 | 0.650× |
| heterogeneous_1m | stringify | 4,325.455 | 905.796 | 1,061.114 | 4.775× |

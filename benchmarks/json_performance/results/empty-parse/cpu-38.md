Current source 1ebaf1f4; median CPU microseconds per call, three fresh processes with default GC. Apple M1 / 8 GiB; Node 26.5.1, Bun 1.3.14. Array-root parsing may defer materialization. Stringify starts fully materialized. Ratios use unrounded CPU timings. Median differences alone do not establish statistical significance.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | 0.724× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.720× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.331× |
| empty_object | parse | 0.058 | 0.045 | 0.024 | 2.388× |
| empty_object | stringify | 0.050 | 0.032 | 0.030 | 1.655× |
| tiny_object | parse | 0.357 | 0.082 | 0.045 | 7.891× |
| tiny_object | stringify | 0.094 | 0.037 | 0.039 | 2.533× |
| small_record | parse | 0.671 | 0.343 | 0.252 | 2.667× |
| small_record | stringify | 0.316 | 0.109 | 0.121 | 2.905× |
| object_1k | parse | 0.589 | 0.542 | 0.237 | 2.482× |
| object_1k | stringify | 0.243 | 0.199 | 0.216 | 1.223× |
| records_array_16k | parse | 23.845 | 39.366 | 33.948 | 0.702× |
| records_array_16k | stringify | 24.024 | 13.269 | 23.240 | 1.811× |
| records_array_1m | parse | 1,550.656 | 2,926.167 | 2,140.875 | 0.724× |
| records_array_1m | stringify | 1,550.277 | 842.356 | 969.119 | 1.840× |
| records_object_1m | parse | 4,004.437 | 2,849.211 | 2,136.085 | 1.875× |
| records_object_1m | stringify | 1,555.063 | 841.397 | 970.460 | 1.848× |
| records_array_8m | parse | 13,362.400 | 33,267.400 | 20,926.500 | 0.639× |
| records_array_8m | stringify | 13,400.619 | 6,771.095 | 8,406.286 | 1.979× |
| records_object_8m | parse | 31,372.125 | 29,769.875 | 21,758.250 | 1.442× |
| records_object_8m | stringify | 13,492.100 | 6,807.700 | 8,422.000 | 1.982× |
| records_array_20m | parse | 77,724.667 | 96,482.667 | 56,673.000 | 1.371× |
| records_array_20m | stringify | 86,187.250 | 17,374.500 | 20,957.500 | 4.961× |
| records_object_20m | parse | 77,647.000 | 95,287.333 | 56,883.333 | 1.365× |
| records_object_20m | stringify | 85,966.625 | 17,325.875 | 20,888.750 | 4.962× |
| numbers_1m | parse | 1,714.196 | 3,124.293 | 3,200.576 | 0.549× |
| numbers_1m | stringify | 1,610.365 | 1,980.010 | 2,946.698 | 0.813× |
| long_string_1m | parse | 203.215 | 369.370 | 67.982 | 2.989× |
| long_string_1m | stringify | 173.090 | 105.978 | 97.395 | 1.777× |
| escaped_1m | parse | 2,040.854 | 1,733.927 | 2,083.354 | 1.177× |
| escaped_1m | stringify | 1,904.892 | 1,861.916 | 2,102.157 | 1.023× |
| unicode_1m | parse | 261.347 | 437.623 | 59.056 | 4.425× |
| unicode_1m | stringify | 140.282 | 412.909 | 450.388 | 0.340× |
| wide_1m | parse | 20,882.056 | 4,941.639 | 4,159.528 | 5.020× |
| wide_1m | stringify | 1,728.102 | 6,370.574 | 666.194 | 2.594× |
| heterogeneous_1m | parse | 1,934.987 | 3,866.425 | 2,977.387 | 0.650× |
| heterogeneous_1m | stringify | 4,441.865 | 904.712 | 1,061.221 | 4.910× |

Source 27eeccf79e5dd30ddabb4132445c9781f0411845; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | 0.714× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.718× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.339× |
| empty_object | parse | 0.058 | 0.045 | 0.024 | 2.401× |
| empty_object | stringify | 0.047 | 0.032 | 0.030 | 1.593× |
| tiny_object | parse | 0.121 | 0.083 | 0.046 | 2.656× |
| tiny_object | stringify | 0.091 | 0.037 | 0.039 | 2.432× |
| small_record | parse | 0.669 | 0.365 | 0.252 | 2.652× |
| small_record | stringify | 0.280 | 0.109 | 0.121 | 2.578× |
| object_1k | parse | 0.587 | 0.542 | 0.237 | 2.474× |
| object_1k | stringify | 0.234 | 0.198 | 0.215 | 1.180× |
| records_array_16k | parse | 23.824 | 39.274 | 33.950 | 0.702× |
| records_array_16k | stringify | 23.488 | 13.261 | 23.258 | 1.771× |
| records_array_1m | parse | 1,553.490 | 2,703.115 | 2,136.094 | 0.727× |
| records_array_1m | stringify | 1,514.188 | 843.080 | 970.972 | 1.796× |
| records_object_1m | parse | 3,920.471 | 2,878.100 | 2,137.986 | 1.834× |
| records_object_1m | stringify | 1,509.847 | 853.079 | 971.791 | 1.770× |
| records_array_8m | parse | 13,380.700 | 33,977.900 | 20,777.600 | 0.644× |
| records_array_8m | stringify | 13,113.600 | 6,811.150 | 8,527.000 | 1.925× |
| records_object_8m | parse | 30,463.250 | 30,324.500 | 21,792.250 | 1.398× |
| records_object_8m | stringify | 13,092.650 | 6,796.850 | 8,423.300 | 1.926× |
| records_array_20m | parse | 75,166.667 | 97,059.000 | 56,455.667 | 1.331× |
| records_array_20m | stringify | 81,301.250 | 17,345.750 | 21,072.000 | 4.687× |
| records_object_20m | parse | 75,135.667 | 95,775.333 | 56,935.000 | 1.320× |
| records_object_20m | stringify | 81,354.250 | 17,431.250 | 20,950.375 | 4.667× |
| numbers_1m | parse | 1,709.272 | 3,128.674 | 3,207.663 | 0.546× |
| numbers_1m | stringify | 1,603.760 | 1,980.188 | 2,952.146 | 0.810× |
| long_string_1m | parse | 204.999 | 368.418 | 66.169 | 3.098× |
| long_string_1m | stringify | 173.953 | 105.006 | 97.520 | 1.784× |
| escaped_1m | parse | 2,037.181 | 1,732.614 | 2,083.530 | 1.176× |
| escaped_1m | stringify | 964.432 | 1,892.740 | 2,086.225 | 0.510× |
| unicode_1m | parse | 256.595 | 438.031 | 60.326 | 4.253× |
| unicode_1m | stringify | 141.528 | 413.138 | 451.121 | 0.343× |
| wide_1m | parse | 20,861.250 | 4,970.806 | 4,165.444 | 5.008× |
| wide_1m | stringify | 1,691.046 | 6,345.731 | 665.787 | 2.540× |
| heterogeneous_1m | parse | 1,914.321 | 3,873.346 | 2,981.741 | 0.642× |
| heterogeneous_1m | stringify | 1,988.988 | 905.500 | 1,059.512 | 2.197× |

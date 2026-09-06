Source 7117f9e67d11af140ee857880b9a96af2ed2890e; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | 0.716× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.242× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.722× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.331× |
| empty_object | parse | 0.057 | 0.045 | 0.024 | 2.355× |
| empty_object | stringify | 0.050 | 0.032 | 0.030 | 1.665× |
| tiny_object | parse | 0.351 | 0.082 | 0.045 | 7.751× |
| tiny_object | stringify | 0.094 | 0.037 | 0.040 | 2.534× |
| small_record | parse | 0.666 | 0.361 | 0.252 | 2.644× |
| small_record | stringify | 0.285 | 0.109 | 0.121 | 2.614× |
| object_1k | parse | 0.584 | 0.540 | 0.237 | 2.465× |
| object_1k | stringify | 0.241 | 0.199 | 0.215 | 1.215× |
| records_array_16k | parse | 23.804 | 40.600 | 33.919 | 0.702× |
| records_array_16k | stringify | 23.922 | 13.247 | 23.317 | 1.806× |
| records_array_1m | parse | 1,552.594 | 2,668.240 | 2,136.042 | 0.727× |
| records_array_1m | stringify | 1,543.833 | 845.799 | 971.218 | 1.825× |
| records_object_1m | parse | 4,000.971 | 2,881.257 | 2,135.671 | 1.873× |
| records_object_1m | stringify | 1,547.358 | 844.369 | 971.068 | 1.833× |
| records_array_8m | parse | 13,425.000 | 33,902.000 | 20,774.200 | 0.646× |
| records_array_8m | stringify | 13,352.476 | 6,794.619 | 8,461.381 | 1.965× |
| records_object_8m | parse | 31,267.375 | 29,759.625 | 21,659.250 | 1.444× |
| records_object_8m | stringify | 13,455.900 | 6,832.150 | 8,366.900 | 1.969× |
| records_array_20m | parse | 76,823.667 | 98,516.667 | 56,760.000 | 1.353× |
| records_array_20m | stringify | 85,729.125 | 17,303.500 | 21,099.375 | 4.954× |
| records_object_20m | parse | 77,447.667 | 95,562.333 | 56,711.000 | 1.366× |
| records_object_20m | stringify | 86,169.250 | 17,337.250 | 21,053.500 | 4.970× |
| numbers_1m | parse | 1,736.022 | 3,122.813 | 3,211.407 | 0.556× |
| numbers_1m | stringify | 1,607.594 | 1,985.167 | 2,951.969 | 0.810× |
| long_string_1m | parse | 205.415 | 368.988 | 67.861 | 3.027× |
| long_string_1m | stringify | 174.557 | 106.535 | 97.698 | 1.787× |
| escaped_1m | parse | 2,040.134 | 1,734.622 | 2,084.366 | 1.176× |
| escaped_1m | stringify | 965.615 | 1,900.361 | 2,088.237 | 0.508× |
| unicode_1m | parse | 258.236 | 437.724 | 60.258 | 4.286× |
| unicode_1m | stringify | 139.312 | 413.033 | 451.201 | 0.337× |
| wide_1m | parse | 20,898.861 | 4,953.333 | 4,169.667 | 5.012× |
| wide_1m | stringify | 1,711.771 | 6,362.000 | 666.131 | 2.570× |
| heterogeneous_1m | parse | 1,933.688 | 3,874.963 | 2,984.162 | 0.648× |
| heterogeneous_1m | stringify | 4,400.414 | 905.728 | 1,060.077 | 4.858× |

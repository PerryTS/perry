Source 2103f41905c95e8174f148df96812303a8b0a520; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | 0.719× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.240× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.717× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.339× |
| empty_object | parse | 0.058 | 0.045 | 0.024 | 2.389× |
| empty_object | stringify | 0.047 | 0.032 | 0.030 | 1.586× |
| tiny_object | parse | 0.121 | 0.080 | 0.045 | 2.686× |
| tiny_object | stringify | 0.091 | 0.037 | 0.039 | 2.433× |
| small_record | parse | 0.670 | 0.331 | 0.253 | 2.653× |
| small_record | stringify | 0.281 | 0.108 | 0.120 | 2.592× |
| object_1k | parse | 0.594 | 0.545 | 0.237 | 2.509× |
| object_1k | stringify | 0.233 | 0.198 | 0.215 | 1.173× |
| records_array_16k | parse | 23.797 | 41.938 | 34.068 | 0.699× |
| records_array_16k | stringify | 23.368 | 13.166 | 23.277 | 1.775× |
| records_array_1m | parse | 1,553.250 | 2,773.323 | 2,134.948 | 0.728× |
| records_array_1m | stringify | 1,497.523 | 847.528 | 969.108 | 1.767× |
| records_object_1m | parse | 3,926.690 | 2,733.577 | 2,137.239 | 1.837× |
| records_object_1m | stringify | 1,502.069 | 842.469 | 971.349 | 1.783× |
| records_array_8m | parse | 13,331.800 | 34,322.700 | 20,870.800 | 0.639× |
| records_array_8m | stringify | 12,952.095 | 6,794.524 | 8,459.143 | 1.906× |
| records_object_8m | parse | 30,529.000 | 30,059.625 | 21,734.125 | 1.405× |
| records_object_8m | stringify | 13,041.450 | 6,805.850 | 8,424.050 | 1.916× |
| records_array_20m | parse | 75,642.333 | 94,071.667 | 56,775.667 | 1.332× |
| records_array_20m | stringify | 81,800.125 | 17,356.500 | 20,763.375 | 4.713× |
| records_object_20m | parse | 75,327.000 | 95,569.333 | 56,702.667 | 1.328× |
| records_object_20m | stringify | 81,707.625 | 17,334.125 | 20,878.875 | 4.714× |
| numbers_1m | parse | 1,716.549 | 3,126.407 | 3,203.154 | 0.549× |
| numbers_1m | stringify | 1,607.021 | 1,982.385 | 2,952.479 | 0.811× |
| long_string_1m | parse | 204.122 | 368.936 | 66.153 | 3.086× |
| long_string_1m | stringify | 173.972 | 105.006 | 97.663 | 1.781× |
| escaped_1m | parse | 2,044.268 | 1,732.927 | 2,086.488 | 1.180× |
| escaped_1m | stringify | 951.627 | 1,895.917 | 2,084.947 | 0.502× |
| unicode_1m | parse | 257.038 | 438.131 | 60.552 | 4.245× |
| unicode_1m | stringify | 140.331 | 413.633 | 449.318 | 0.339× |
| wide_1m | parse | 20,859.778 | 4,936.722 | 4,168.056 | 5.005× |
| wide_1m | stringify | 1,691.603 | 6,363.995 | 665.070 | 2.543× |
| heterogeneous_1m | parse | 1,911.802 | 3,962.506 | 2,961.210 | 0.646× |
| heterogeneous_1m | stringify | 843.352 | 908.614 | 1,054.114 | 0.928× |

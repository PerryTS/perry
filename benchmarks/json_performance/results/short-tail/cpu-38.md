Source 090e120f3773e78aa699bcf87a12eb15d38ec3b3; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.013 | 0.027 | 0.019 | 0.682× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.691× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.338× |
| empty_object | parse | 0.057 | 0.045 | 0.024 | 2.341× |
| empty_object | stringify | 0.050 | 0.032 | 0.030 | 1.670× |
| tiny_object | parse | 0.361 | 0.081 | 0.046 | 7.926× |
| tiny_object | stringify | 0.094 | 0.037 | 0.040 | 2.534× |
| small_record | parse | 0.679 | 0.348 | 0.252 | 2.691× |
| small_record | stringify | 0.288 | 0.109 | 0.121 | 2.650× |
| object_1k | parse | 0.602 | 0.542 | 0.237 | 2.538× |
| object_1k | stringify | 0.241 | 0.200 | 0.214 | 1.204× |
| records_array_16k | parse | 23.342 | 40.489 | 33.950 | 0.688× |
| records_array_16k | stringify | 23.729 | 13.419 | 23.264 | 1.768× |
| records_array_1m | parse | 1,522.144 | 2,737.691 | 2,134.577 | 0.713× |
| records_array_1m | stringify | 1,522.830 | 844.574 | 969.977 | 1.803× |
| records_object_1m | parse | 3,991.662 | 2,825.028 | 2,135.761 | 1.869× |
| records_object_1m | stringify | 1,526.633 | 844.729 | 970.356 | 1.807× |
| records_array_8m | parse | 13,174.600 | 35,562.300 | 20,990.700 | 0.628× |
| records_array_8m | stringify | 13,289.850 | 6,792.650 | 8,475.700 | 1.957× |
| records_object_8m | parse | 30,960.571 | 29,110.429 | 20,536.429 | 1.508× |
| records_object_8m | stringify | 13,308.550 | 6,787.000 | 8,429.450 | 1.961× |
| records_array_20m | parse | 77,442.000 | 97,607.333 | 56,495.667 | 1.371× |
| records_array_20m | stringify | 85,588.250 | 17,328.875 | 20,778.625 | 4.939× |
| records_object_20m | parse | 77,171.667 | 94,978.333 | 56,738.667 | 1.360× |
| records_object_20m | stringify | 85,505.000 | 17,331.875 | 20,843.875 | 4.933× |
| numbers_1m | parse | 1,726.891 | 3,125.859 | 3,204.141 | 0.552× |
| numbers_1m | stringify | 1,606.948 | 1,970.104 | 2,948.906 | 0.816× |
| long_string_1m | parse | 212.055 | 368.364 | 66.461 | 3.191× |
| long_string_1m | stringify | 173.473 | 106.024 | 97.527 | 1.779× |
| escaped_1m | parse | 1,974.795 | 1,733.096 | 2,083.602 | 1.139× |
| escaped_1m | stringify | 965.479 | 1,903.355 | 2,087.497 | 0.507× |
| unicode_1m | parse | 263.026 | 438.212 | 60.157 | 4.372× |
| unicode_1m | stringify | 139.110 | 413.746 | 451.111 | 0.336× |
| wide_1m | parse | 20,882.222 | 4,967.139 | 4,164.583 | 5.014× |
| wide_1m | stringify | 1,708.488 | 6,343.451 | 665.944 | 2.566× |
| heterogeneous_1m | parse | 1,880.639 | 3,952.627 | 2,973.024 | 0.633× |
| heterogeneous_1m | stringify | 4,285.313 | 905.506 | 1,062.970 | 4.733× |

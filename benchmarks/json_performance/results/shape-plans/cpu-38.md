Source 936c60675bcf8c4cc65d76f48c96937338a4e620; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | 0.715× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.720× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.339× |
| empty_object | parse | 0.058 | 0.045 | 0.024 | 2.398× |
| empty_object | stringify | 0.047 | 0.032 | 0.030 | 1.572× |
| tiny_object | parse | 0.120 | 0.080 | 0.045 | 2.658× |
| tiny_object | stringify | 0.091 | 0.037 | 0.040 | 2.444× |
| small_record | parse | 0.672 | 0.330 | 0.253 | 2.652× |
| small_record | stringify | 0.281 | 0.108 | 0.121 | 2.590× |
| object_1k | parse | 0.595 | 0.544 | 0.237 | 2.507× |
| object_1k | stringify | 0.234 | 0.199 | 0.215 | 1.177× |
| records_array_16k | parse | 23.673 | 39.624 | 33.951 | 0.697× |
| records_array_16k | stringify | 23.226 | 13.242 | 23.242 | 1.754× |
| records_array_1m | parse | 1,551.448 | 2,893.427 | 2,144.156 | 0.724× |
| records_array_1m | stringify | 1,494.909 | 844.583 | 969.526 | 1.770× |
| records_object_1m | parse | 3,905.671 | 2,789.414 | 2,191.600 | 1.782× |
| records_object_1m | stringify | 1,493.808 | 848.525 | 970.271 | 1.760× |
| records_array_8m | parse | 13,358.500 | 33,868.000 | 20,821.200 | 0.642× |
| records_array_8m | stringify | 13,007.850 | 6,843.700 | 8,554.350 | 1.901× |
| records_object_8m | parse | 30,175.000 | 30,787.750 | 21,790.625 | 1.385× |
| records_object_8m | stringify | 12,882.857 | 6,804.381 | 8,469.429 | 1.893× |
| records_array_20m | parse | 74,625.000 | 97,020.000 | 56,712.667 | 1.316× |
| records_array_20m | stringify | 81,213.375 | 17,303.125 | 20,833.875 | 4.694× |
| records_object_20m | parse | 74,569.000 | 94,019.667 | 56,559.333 | 1.318× |
| records_object_20m | stringify | 81,344.250 | 17,313.500 | 20,806.750 | 4.698× |
| numbers_1m | parse | 1,740.670 | 3,118.769 | 3,199.505 | 0.558× |
| numbers_1m | stringify | 1,643.010 | 1,986.240 | 2,949.146 | 0.827× |
| long_string_1m | parse | 204.101 | 368.514 | 65.947 | 3.095× |
| long_string_1m | stringify | 172.193 | 105.343 | 97.151 | 1.772× |
| escaped_1m | parse | 2,042.061 | 1,734.220 | 2,083.927 | 1.178× |
| escaped_1m | stringify | 939.771 | 1,900.777 | 2,085.253 | 0.494× |
| unicode_1m | parse | 257.304 | 438.301 | 59.284 | 4.340× |
| unicode_1m | stringify | 138.708 | 412.333 | 448.629 | 0.336× |
| wide_1m | parse | 20,910.972 | 4,941.500 | 4,159.306 | 5.028× |
| wide_1m | stringify | 1,707.000 | 6,336.648 | 665.250 | 2.566× |
| heterogeneous_1m | parse | 1,909.815 | 3,859.185 | 2,956.012 | 0.646× |
| heterogeneous_1m | stringify | 1,353.539 | 923.820 | 1,060.934 | 1.465× |

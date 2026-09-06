Source 9187bd95d9ff6ddb541a0f548203c47d060a76dd; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.013 | 0.027 | 0.019 | 0.682× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.691× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.337× |
| empty_object | parse | 0.056 | 0.045 | 0.024 | 2.325× |
| empty_object | stringify | 0.048 | 0.032 | 0.030 | 1.601× |
| tiny_object | parse | 0.120 | 0.082 | 0.045 | 2.645× |
| tiny_object | stringify | 0.091 | 0.037 | 0.040 | 2.447× |
| small_record | parse | 0.679 | 0.350 | 0.252 | 2.694× |
| small_record | stringify | 0.281 | 0.109 | 0.121 | 2.584× |
| object_1k | parse | 0.591 | 0.541 | 0.237 | 2.490× |
| object_1k | stringify | 0.235 | 0.199 | 0.216 | 1.179× |
| records_array_16k | parse | 24.021 | 40.154 | 33.998 | 0.707× |
| records_array_16k | stringify | 23.223 | 13.243 | 23.306 | 1.754× |
| records_array_1m | parse | 1,561.600 | 2,700.032 | 2,132.800 | 0.732× |
| records_array_1m | stringify | 1,498.109 | 849.943 | 970.339 | 1.763× |
| records_object_1m | parse | 4,041.571 | 2,829.857 | 2,131.543 | 1.896× |
| records_object_1m | stringify | 1,496.775 | 844.697 | 969.758 | 1.772× |
| records_array_8m | parse | 13,485.100 | 33,649.100 | 20,854.900 | 0.647× |
| records_array_8m | stringify | 13,013.524 | 6,813.286 | 8,430.381 | 1.910× |
| records_object_8m | parse | 31,373.375 | 29,720.625 | 21,686.375 | 1.447× |
| records_object_8m | stringify | 13,112.850 | 6,784.400 | 8,440.400 | 1.933× |
| records_array_20m | parse | 77,953.333 | 94,692.000 | 52,766.667 | 1.477× |
| records_array_20m | stringify | 84,945.250 | 17,405.750 | 20,955.625 | 4.880× |
| records_object_20m | parse | 77,454.000 | 96,901.667 | 57,057.333 | 1.357× |
| records_object_20m | stringify | 85,125.250 | 17,370.250 | 20,918.375 | 4.901× |
| numbers_1m | parse | 1,713.304 | 3,125.793 | 3,202.293 | 0.548× |
| numbers_1m | stringify | 1,602.823 | 1,982.688 | 2,951.000 | 0.808× |
| long_string_1m | parse | 204.075 | 368.631 | 67.572 | 3.020× |
| long_string_1m | stringify | 175.766 | 105.808 | 96.014 | 1.831× |
| escaped_1m | parse | 2,041.646 | 1,734.012 | 2,083.963 | 1.177× |
| escaped_1m | stringify | 968.107 | 1,899.643 | 2,086.286 | 0.510× |
| unicode_1m | parse | 258.573 | 438.204 | 59.454 | 4.349× |
| unicode_1m | stringify | 139.437 | 412.143 | 451.198 | 0.338× |
| wide_1m | parse | 20,956.222 | 4,954.694 | 4,166.556 | 5.030× |
| wide_1m | stringify | 1,691.995 | 6,340.355 | 665.710 | 2.542× |
| heterogeneous_1m | parse | 1,940.812 | 3,924.225 | 2,998.875 | 0.647× |
| heterogeneous_1m | stringify | 4,358.304 | 904.869 | 1,061.690 | 4.817× |

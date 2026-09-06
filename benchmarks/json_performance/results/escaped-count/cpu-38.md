Source 4b9d577d4e81518f83a8b4a1e4f319d328597f74; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | 0.712× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.716× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.338× |
| empty_object | parse | 0.059 | 0.045 | 0.024 | 2.453× |
| empty_object | stringify | 0.050 | 0.032 | 0.030 | 1.667× |
| tiny_object | parse | 0.360 | 0.080 | 0.045 | 7.911× |
| tiny_object | stringify | 0.095 | 0.037 | 0.040 | 2.565× |
| small_record | parse | 0.675 | 0.341 | 0.252 | 2.683× |
| small_record | stringify | 0.324 | 0.108 | 0.121 | 2.985× |
| object_1k | parse | 0.593 | 0.542 | 0.237 | 2.496× |
| object_1k | stringify | 0.243 | 0.199 | 0.215 | 1.226× |
| records_array_16k | parse | 23.811 | 39.345 | 33.960 | 0.701× |
| records_array_16k | stringify | 23.928 | 13.232 | 23.228 | 1.808× |
| records_array_1m | parse | 1,549.990 | 2,837.969 | 2,142.510 | 0.723× |
| records_array_1m | stringify | 1,546.966 | 847.695 | 970.736 | 1.825× |
| records_object_1m | parse | 4,011.563 | 2,786.000 | 2,134.972 | 1.879× |
| records_object_1m | stringify | 1,540.697 | 847.843 | 968.966 | 1.817× |
| records_array_8m | parse | 13,402.900 | 33,849.900 | 20,713.700 | 0.647× |
| records_array_8m | stringify | 13,431.300 | 6,774.200 | 8,507.050 | 1.983× |
| records_object_8m | parse | 31,345.750 | 30,433.000 | 21,697.750 | 1.445× |
| records_object_8m | stringify | 13,423.650 | 6,824.250 | 8,517.150 | 1.967× |
| records_array_20m | parse | 77,847.333 | 95,739.333 | 56,707.000 | 1.373× |
| records_array_20m | stringify | 85,897.250 | 17,322.625 | 20,848.500 | 4.959× |
| records_object_20m | parse | 77,820.000 | 98,168.333 | 56,830.667 | 1.369× |
| records_object_20m | stringify | 85,779.750 | 17,335.875 | 21,039.000 | 4.948× |
| numbers_1m | parse | 1,740.600 | 3,125.622 | 3,201.333 | 0.557× |
| numbers_1m | stringify | 1,602.354 | 1,978.896 | 2,955.188 | 0.810× |
| long_string_1m | parse | 206.238 | 369.228 | 68.467 | 3.012× |
| long_string_1m | stringify | 175.872 | 113.099 | 98.021 | 1.794× |
| escaped_1m | parse | 2,043.049 | 1,733.024 | 2,084.878 | 1.179× |
| escaped_1m | stringify | 952.935 | 1,898.444 | 2,086.521 | 0.502× |
| unicode_1m | parse | 257.794 | 438.278 | 59.222 | 4.353× |
| unicode_1m | stringify | 139.539 | 413.382 | 450.651 | 0.338× |
| wide_1m | parse | 20,974.833 | 4,994.222 | 4,176.472 | 5.022× |
| wide_1m | stringify | 1,691.801 | 6,336.940 | 667.264 | 2.535× |
| heterogeneous_1m | parse | 1,933.235 | 3,924.568 | 2,958.062 | 0.654× |
| heterogeneous_1m | stringify | 4,395.695 | 921.647 | 1,060.886 | 4.769× |

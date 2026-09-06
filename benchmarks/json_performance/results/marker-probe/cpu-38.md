Source 7be2932faa3ca9e98dcfe6d89f9781b7360ab063; median CPU microseconds per call, three fresh processes, default GC. Apple M1 / 8 GiB; Node 26.5.1 and Bun 1.3.14. Array-root parsing may defer materialization; stringify starts fully materialized. Ratios use unrounded timings.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.013 | 0.027 | 0.019 | 0.701× |
| null | stringify | 0.007 | 0.027 | 0.029 | 0.241× |
| string_a | parse | 0.016 | 0.032 | 0.023 | 0.707× |
| string_a | stringify | 0.010 | 0.030 | 0.029 | 0.337× |
| empty_object | parse | 0.057 | 0.045 | 0.024 | 2.346× |
| empty_object | stringify | 0.047 | 0.032 | 0.030 | 1.571× |
| tiny_object | parse | 0.363 | 0.082 | 0.045 | 8.026× |
| tiny_object | stringify | 0.091 | 0.037 | 0.039 | 2.453× |
| small_record | parse | 0.687 | 0.346 | 0.255 | 2.696× |
| small_record | stringify | 0.282 | 0.109 | 0.121 | 2.592× |
| object_1k | parse | 0.598 | 0.541 | 0.238 | 2.512× |
| object_1k | stringify | 0.234 | 0.199 | 0.216 | 1.172× |
| records_array_16k | parse | 23.802 | 40.306 | 33.963 | 0.701× |
| records_array_16k | stringify | 23.247 | 13.237 | 23.185 | 1.756× |
| records_array_1m | parse | 1,555.606 | 2,730.330 | 2,134.840 | 0.729× |
| records_array_1m | stringify | 1,500.023 | 844.170 | 970.205 | 1.777× |
| records_object_1m | parse | 4,040.261 | 2,763.812 | 2,138.565 | 1.889× |
| records_object_1m | stringify | 1,506.199 | 842.426 | 969.023 | 1.788× |
| records_array_8m | parse | 13,368.100 | 34,869.700 | 20,797.700 | 0.643× |
| records_array_8m | stringify | 13,096.500 | 6,819.050 | 8,523.700 | 1.921× |
| records_object_8m | parse | 31,354.000 | 29,850.000 | 21,743.250 | 1.442× |
| records_object_8m | stringify | 13,133.400 | 6,827.300 | 8,442.550 | 1.924× |
| records_array_20m | parse | 77,229.333 | 94,743.000 | 56,734.333 | 1.361× |
| records_array_20m | stringify | 85,444.875 | 17,333.375 | 20,760.000 | 4.930× |
| records_object_20m | parse | 77,677.333 | 97,056.667 | 56,790.333 | 1.368× |
| records_object_20m | stringify | 85,245.875 | 17,361.000 | 20,736.250 | 4.910× |
| numbers_1m | parse | 1,711.598 | 3,121.457 | 3,214.783 | 0.548× |
| numbers_1m | stringify | 1,606.442 | 1,978.495 | 2,953.789 | 0.812× |
| long_string_1m | parse | 208.535 | 369.554 | 68.110 | 3.062× |
| long_string_1m | stringify | 174.485 | 105.718 | 96.162 | 1.814× |
| escaped_1m | parse | 2,043.098 | 1,733.561 | 2,083.439 | 1.179× |
| escaped_1m | stringify | 951.101 | 1,899.491 | 2,088.521 | 0.501× |
| unicode_1m | parse | 258.699 | 438.044 | 59.206 | 4.369× |
| unicode_1m | stringify | 139.402 | 412.555 | 450.279 | 0.338× |
| wide_1m | parse | 20,869.861 | 4,953.972 | 4,173.333 | 5.001× |
| wide_1m | stringify | 1,693.343 | 6,319.056 | 666.840 | 2.539× |
| heterogeneous_1m | parse | 1,935.575 | 3,873.600 | 2,957.688 | 0.654× |
| heterogeneous_1m | stringify | 4,373.119 | 904.542 | 1,059.661 | 4.835× |

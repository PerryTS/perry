Latest candidate cc1c4f5; Apple M1, 8 GiB; Node 26.5.1, Bun 1.3.14. Median CPU microseconds per call (user + system), three fresh processes with default GC. Ratios use unrounded timings. Above 1 means Perry takes more CPU than the faster comparison engine. Array-root parsing may return a lazy value; deferred materialization is not included. Stringify uses fully materialized inputs. Median differences alone do not establish significance.

| Fixture | Operation | Perry µs | Node µs | Bun µs | Perry ÷ best |
|---|---|---:|---:|---:|---:|
| null | parse | 0.014 | 0.027 | 0.019 | **0.72×** |
| null | stringify | 0.007 | 0.027 | 0.029 | **0.24×** |
| string_a | parse | 0.018 | 0.032 | 0.023 | **0.77×** |
| string_a | stringify | 0.010 | 0.030 | 0.029 | **0.34×** |
| empty_object | parse | 0.322 | 0.045 | 0.024 | 13.30× |
| empty_object | stringify | 0.050 | 0.032 | 0.030 | 1.65× |
| tiny_object | parse | 0.357 | 0.081 | 0.045 | 7.90× |
| tiny_object | stringify | 0.095 | 0.037 | 0.039 | 2.55× |
| small_record | parse | 0.676 | 0.349 | 0.255 | 2.65× |
| small_record | stringify | 0.315 | 0.109 | 0.121 | 2.90× |
| object_1k | parse | 0.595 | 0.540 | 0.237 | 2.51× |
| object_1k | stringify | 0.245 | 0.199 | 0.215 | 1.23× |
| records_array_16k | parse | 23.804 | 39.162 | 33.944 | **0.70×** |
| records_array_16k | stringify | 24.206 | 13.269 | 23.252 | 1.82× |
| records_array_1m | parse | 1,550.309 | 2,681.043 | 2,135.660 | **0.73×** |
| records_array_1m | stringify | 1,551.757 | 849.384 | 970.514 | 1.83× |
| records_object_1m | parse | 4,032.486 | 2,671.929 | 2,137.286 | 1.89× |
| records_object_1m | stringify | 1,552.349 | 845.566 | 973.874 | 1.84× |
| records_array_8m | parse | 13,376.500 | 34,204.900 | 20,810.200 | **0.64×** |
| records_array_8m | stringify | 13,495.350 | 6,831.500 | 8,451.350 | 1.98× |
| records_object_8m | parse | 31,389.250 | 29,607.625 | 21,748.750 | 1.44× |
| records_object_8m | stringify | 13,456.950 | 6,811.800 | 8,451.400 | 1.98× |
| records_array_20m | parse | 77,287.333 | 99,126.333 | 56,735.333 | 1.36× |
| records_array_20m | stringify | 86,059.500 | 17,342.375 | 21,028.875 | 4.96× |
| records_object_20m | parse | 77,381.333 | 97,830.667 | 56,661.333 | 1.37× |
| records_object_20m | stringify | 85,879.750 | 17,300.250 | 20,830.625 | 4.96× |
| numbers_1m | parse | 1,717.692 | 3,125.879 | 3,190.527 | **0.55×** |
| numbers_1m | stringify | 1,606.347 | 1,983.779 | 2,948.905 | **0.81×** |
| long_string_1m | parse | 206.211 | 368.885 | 67.686 | 3.05× |
| long_string_1m | stringify | 172.653 | 105.604 | 96.992 | 1.78× |
| escaped_1m | parse | 2,047.659 | 1,733.610 | 2,086.939 | 1.18× |
| escaped_1m | stringify | 1,861.313 | 1,867.699 | 2,089.410 | **0.997×** |
| unicode_1m | parse | 256.371 | 437.664 | 60.541 | 4.23× |
| unicode_1m | stringify | 138.934 | 412.581 | 451.754 | **0.34×** |
| wide_1m | parse | 20,868.861 | 4,962.444 | 4,162.750 | 5.01× |
| wide_1m | stringify | 1,690.870 | 6,358.255 | 666.417 | 2.54× |
| heterogeneous_1m | parse | 1,936.000 | 3,849.537 | 2,973.350 | **0.65×** |
| heterogeneous_1m | stringify | 4,430.862 | 906.395 | 1,060.078 | 4.89× |

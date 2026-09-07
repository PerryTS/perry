# All 38 CPU rows

CPU microseconds per call, including GC in the measured loop. Medians of three fresh processes; qualified window. Parent is the construction batch. Array-root parse may be lazy; stringify starts from eager input.

| Fixture | Operation | Parent | Deferred | Node | Bun | Change vs parent |
|---|---|---:|---:|---:|---:|---:|
| null | parse | 0.014 | 0.014 | 0.027 | 0.019 | -0.03% |
| null | stringify | 0.007 | 0.007 | 0.027 | 0.029 | -0.08% |
| string_a | parse | 0.016 | 0.016 | 0.032 | 0.023 | -0.04% |
| string_a | stringify | 0.010 | 0.010 | 0.030 | 0.029 | +0.04% |
| empty_object | parse | 0.057 | 0.057 | 0.045 | 0.024 | -0.14% |
| empty_object | stringify | 0.047 | 0.047 | 0.032 | 0.030 | +0.87% |
| tiny_object | parse | 0.122 | 0.122 | 0.082 | 0.045 | -0.14% |
| tiny_object | stringify | 0.093 | 0.093 | 0.037 | 0.039 | +0.64% |
| small_record | parse | 0.543 | 0.540 | 0.356 | 0.253 | -0.62% |
| small_record | stringify | 0.284 | 0.284 | 0.109 | 0.121 | +0.16% |
| object_1k | parse | 0.542 | 0.541 | 0.543 | 0.237 | -0.18% |
| object_1k | stringify | 0.237 | 0.237 | 0.199 | 0.215 | +0.06% |
| records_array_16k | parse | 23.835 | 23.877 | 39.775 | 33.940 | +0.18% |
| records_array_16k | stringify | 23.591 | 23.404 | 13.197 | 23.217 | -0.79% |
| records_array_1m | parse | 1557.916 | 1551.368 | 2719.074 | 2133.695 | -0.42% |
| records_array_1m | stringify | 1496.814 | 1507.972 | 843.316 | 968.763 | +0.75% |
| records_object_1m | parse | 2835.042 | 2823.042 | 2773.930 | 2132.944 | -0.42% |
| records_object_1m | stringify | 1495.994 | 1511.236 | 841.644 | 971.977 | +1.02% |
| records_array_8m | parse | 13387.800 | 13404.900 | 35203.900 | 20820.200 | +0.13% |
| records_array_8m | stringify | 12962.650 | 13028.150 | 6791.800 | 8454.700 | +0.51% |
| records_object_8m | parse | 21609.125 | 21483.500 | 29457.250 | 21760.375 | -0.58% |
| records_object_8m | stringify | 12869.762 | 12988.476 | 6808.095 | 8471.905 | +0.92% |
| records_array_20m | parse | 53341.333 | 52899.000 | 96497.667 | 56723.000 | -0.83% |
| records_array_20m | stringify | 81785.000 | 82190.000 | 17363.125 | 20818.750 | +0.50% |
| records_object_20m | parse | 53354.667 | 52904.667 | 96428.000 | 56561.667 | -0.84% |
| records_object_20m | stringify | 82051.500 | 82050.625 | 17303.875 | 20925.250 | -0.00% |
| numbers_1m | parse | 1738.228 | 1715.837 | 3125.250 | 3198.913 | -1.29% |
| numbers_1m | stringify | 1609.646 | 1604.010 | 1980.802 | 2956.365 | -0.35% |
| long_string_1m | parse | 204.501 | 203.463 | 368.655 | 66.021 | -0.51% |
| long_string_1m | stringify | 173.852 | 174.393 | 105.877 | 97.797 | +0.31% |
| escaped_1m | parse | 2039.939 | 2041.280 | 1733.683 | 2085.354 | +0.07% |
| escaped_1m | stringify | 951.685 | 951.685 | 1894.958 | 2086.482 | +0.00% |
| unicode_1m | parse | 258.484 | 257.013 | 437.687 | 60.661 | -0.57% |
| unicode_1m | stringify | 139.892 | 139.662 | 412.677 | 450.919 | -0.16% |
| wide_1m | parse | 20399.056 | 15180.139 | 4920.000 | 4166.333 | -25.58% |
| wide_1m | stringify | 1692.874 | 1708.209 | 6337.674 | 666.205 | +0.91% |
| heterogeneous_1m | parse | 1935.338 | 1935.612 | 3886.088 | 2953.262 | +0.01% |
| heterogeneous_1m | stringify | 848.109 | 848.720 | 902.673 | 1053.900 | +0.07% |

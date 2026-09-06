Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.251 / 0.013 / 0.027 / 0.019 | 19.06× | 13.55 / 13.45 / 57.55 / 35.70 |
| null / stringify | 2,000,000 | 0.066 / 0.007 / 0.027 / 0.029 | 10.03× | 34.88 / 13.58 / 59.55 / 129.75 |
| string_a / parse | 2,000,000 | 0.254 / 0.016 / 0.032 / 0.023 | 16.28× | 13.56 / 13.44 / 57.59 / 36.03 |
| string_a / stringify | 2,000,000 | 0.074 / 0.010 / 0.030 / 0.029 | 7.68× | 34.86 / 13.58 / 59.45 / 129.77 |
| empty_object / parse | 2,000,000 | 0.326 / 0.328 / 0.045 / 0.024 | 0.99× | 34.64 / 34.53 / 59.52 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.137 / 0.056 / 0.032 / 0.030 | 2.46× | 34.92 / 13.62 / 59.53 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.363 / 0.366 / 0.082 / 0.045 | 0.99× | 34.64 / 34.53 / 59.55 / 69.05 |
| tiny_object / stringify | 2,000,000 | 0.197 / 0.141 / 0.037 / 0.040 | 1.40× | 34.92 / 34.77 / 59.59 / 129.75 |
| small_record / parse | 582,665 | 0.751 / 0.748 / 0.338 / 0.253 | 1.00× | 35.00 / 34.91 / 59.55 / 79.84 |
| small_record / stringify | 1,257,861 | 0.550 / 0.530 / 0.109 / 0.121 | 1.04× | 35.98 / 35.91 / 59.61 / 251.89 |
| object_1k / parse | 539,730 | 0.590 / 0.592 / 0.542 / 0.237 | 1.00× | 35.05 / 34.92 / 61.64 / 71.09 |
| object_1k / stringify | 636,155 | 0.386 / 0.288 / 0.199 / 0.215 | 1.34× | 36.06 / 35.89 / 61.61 / 70.80 |
| records_array_16k / parse | 6,735 | 23.762 / 23.836 / 39.740 / 33.999 | 1.00× | 65.02 / 64.92 / 65.77 / 70.45 |
| records_array_16k / stringify | 9,944 | 35.719 / 25.143 / 13.240 / 23.218 | 1.42× | 36.28 / 36.16 / 61.80 / 71.02 |
| records_array_1m / parse | 93 | 1,541.688 / 1,520.290 / 2,780.301 / 2,140.785 | 1.01× | 62.31 / 62.19 / 92.78 / 79.09 |
| records_array_1m / stringify | 175 | 2,205.046 / 1,609.211 / 845.057 / 969.737 | 1.37× | 60.56 / 60.44 / 107.50 / 100.30 |
| records_object_1m / parse | 70 | 4,020.257 / 4,007.657 / 2,926.086 / 2,138.500 | 1.00× | 80.14 / 80.02 / 92.78 / 75.91 |
| records_object_1m / stringify | 176 | 2,198.301 / 1,612.256 / 843.500 / 969.972 | 1.36× | 60.55 / 60.42 / 108.30 / 100.27 |
| records_array_8m / parse | 10 | 13,815.900 / 13,626.000 / 34,956.100 / 20,864.500 | 1.01× | 102.75 / 102.62 / 244.12 / 131.56 |
| records_array_8m / stringify | 20 | 18,731.650 / 13,930.000 / 6,838.800 / 8,456.800 | 1.34× | 196.03 / 195.98 / 185.97 / 169.62 |
| records_object_8m / parse | 8 | 31,694.875 / 31,578.125 / 30,008.000 / 21,804.250 | 1.00× | 231.81 / 231.77 / 225.47 / 111.66 |
| records_object_8m / stringify | 20 | 18,712.900 / 13,935.350 / 6,846.350 / 8,521.200 | 1.34× | 196.05 / 195.98 / 186.02 / 169.78 |
| records_array_20m / parse | 3 | 78,691.333 / 77,907.667 / 98,252.333 / 56,714.000 | 1.01× | 315.83 / 315.77 / 325.81 / 188.44 |
| records_array_20m / stringify | 8 | 97,533.750 / 87,096.500 / 17,409.500 / 20,813.625 | 1.12× | 171.38 / 171.25 / 392.16 / 304.94 |
| records_object_20m / parse | 3 | 78,764.000 / 78,404.000 / 96,804.667 / 56,740.000 | 1.00× | 315.83 / 315.77 / 326.05 / 188.08 |
| records_object_20m / stringify | 8 | 97,819.000 / 87,030.375 / 17,390.000 / 20,858.125 | 1.12× | 171.34 / 171.25 / 392.12 / 304.88 |
| numbers_1m / parse | 96 | 1,574.854 / 1,700.573 / 3,122.833 / 3,194.469 | 0.93× | 64.52 / 64.45 / 97.92 / 68.19 |
| numbers_1m / stringify | 96 | 6,579.969 / 1,606.646 / 1,980.458 / 2,953.010 | 4.10× | 61.44 / 61.31 / 92.36 / 72.22 |
| long_string_1m / parse | 1,344 | 203.240 / 206.371 / 367.862 / 67.337 | 0.98× | 60.92 / 60.80 / 160.95 / 153.44 |
| long_string_1m / stringify | 1,155 | 204.984 / 173.809 / 105.320 / 95.899 | 1.18× | 64.02 / 61.98 / 157.94 / 123.56 |
| escaped_1m / parse | 82 | 2,042.085 / 2,055.476 / 1,733.537 / 2,082.500 | 0.99× | 58.30 / 58.17 / 74.06 / 64.19 |
| escaped_1m / stringify | 83 | 1,860.639 / 1,862.108 / 1,856.024 / 2,092.265 | 1.00× | 57.08 / 57.00 / 79.52 / 68.95 |
| unicode_1m / parse | 1,442 | 257.246 / 258.683 / 438.056 / 59.077 | 0.99× | 55.64 / 55.50 / 164.11 / 123.11 |
| unicode_1m / stringify | 1,311 | 254.228 / 139.151 / 413.641 / 449.924 | 1.83× | 58.41 / 57.25 / 154.94 / 160.62 |
| wide_1m / parse | 36 | 21,016.083 / 20,874.000 / 4,943.222 / 4,156.778 | 1.01× | 176.77 / 176.53 / 110.56 / 86.11 |
| wide_1m / stringify | 216 | 2,470.764 / 2,482.157 / 6,369.593 / 665.162 | 1.00× | 71.00 / 70.91 / 116.44 / 84.55 |
| heterogeneous_1m / parse | 78 | 1,869.923 / 1,919.154 / 3,833.449 / 2,961.385 | 0.97× | 68.19 / 68.08 / 92.12 / 80.91 |
| heterogeneous_1m / stringify | 167 | 4,489.892 / 4,545.317 / 904.850 / 1,063.521 | 0.99× | 59.27 / 59.16 / 110.47 / 101.41 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.33 / 12.22 / 52.67 / 28.30 |
| tiny_object / retain-parse | 200,000 | 24.91 / 24.81 / 74.91 / 50.17 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.36 / 52.69 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 36.41 / 24.94 / 71.17 / 47.00 |
| small_record / retain-parse | 1 | 12.34 / 12.23 / 52.70 / 28.27 |
| small_record / retain-parse | 100,000 | 57.00 / 56.88 / 85.52 / 52.67 |
| small_record / retain-stringify | 1 | 12.61 / 12.48 / 52.75 / 28.31 |
| small_record / retain-stringify | 100,000 | 31.44 / 31.34 / 80.33 / 51.05 |
| records_array_1m / retain-parse | 1 | 17.97 / 17.84 / 58.31 / 31.61 |
| records_array_1m / retain-parse | 16 | 41.38 / 41.27 / 88.64 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.17 / 20.02 / 60.50 / 34.42 |
| records_array_1m / retain-stringify | 16 | 34.06 / 33.92 / 76.30 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.98 / 15.89 / 58.38 / 31.61 |
| records_object_1m / retain-parse | 16 | 67.14 / 66.95 / 88.67 / 55.91 |
| records_object_1m / retain-stringify | 1 | 20.17 / 20.02 / 60.50 / 34.42 |
| records_object_1m / retain-stringify | 16 | 34.06 / 33.92 / 76.33 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.62 / 48.48 / 94.28 / 52.31 |
| records_array_8m / retain-parse | 4 | 89.30 / 89.11 / 146.86 / 91.81 |
| records_array_8m / retain-stringify | 1 | 75.77 / 75.59 / 114.17 / 73.45 |
| records_array_8m / retain-stringify | 4 | 95.06 / 94.89 / 143.92 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.95 / 42.89 / 94.27 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.44 / 113.33 / 146.89 / 91.92 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.61 / 114.23 / 73.44 |
| records_object_8m / retain-stringify | 4 | 95.06 / 94.91 / 144.06 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.44 / 15.34 / 56.47 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.98 / 56.80 / 89.16 / 61.62 |
| long_string_1m / retain-stringify | 1 | 21.16 / 18.83 / 60.58 / 32.59 |
| long_string_1m / retain-stringify | 32 | 59.89 / 58.75 / 92.25 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.62 / 14.55 / 57.67 / 31.23 |
| unicode_1m / retain-parse | 32 | 44.84 / 44.77 / 86.33 / 58.94 |
| unicode_1m / retain-stringify | 1 | 18.73 / 16.81 / 60.78 / 34.12 |
| unicode_1m / retain-stringify | 32 | 48.69 / 46.77 / 89.17 / 61.73 |
| wide_1m / retain-parse | 1 | 24.20 / 24.09 / 65.62 / 36.36 |
| wide_1m / retain-parse | 16 | 71.38 / 71.27 / 112.48 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.28 / 30.28 / 68.58 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.66 / 45.44 / 86.41 / 53.44 |

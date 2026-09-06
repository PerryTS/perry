Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.44 / 13.41 / 57.59 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.50 / 13.47 / 59.55 / 129.75 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.44 / 13.42 / 57.56 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.50 / 13.48 / 59.55 / 129.75 |
| empty_object / parse | 2,000,000 | 0.059 / 0.058 / 0.045 / 0.024 | 1.02× | 34.70 / 34.64 / 59.53 / 69.05 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.62 / 13.59 / 59.52 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.360 / 0.362 / 0.083 / 0.045 | 0.99× | 34.50 / 34.44 / 59.48 / 69.05 |
| tiny_object / stringify | 2,000,000 | 0.095 / 0.096 / 0.037 / 0.040 | 0.99× | 34.72 / 34.66 / 59.64 / 129.73 |
| small_record / parse | 580,363 | 0.676 / 0.677 / 0.376 / 0.252 | 1.00× | 34.86 / 34.80 / 59.50 / 79.88 |
| small_record / stringify | 1,269,169 | 0.321 / 0.333 / 0.109 / 0.121 | 0.96× | 35.86 / 35.80 / 59.64 / 253.83 |
| object_1k / parse | 563,380 | 0.593 / 0.597 / 0.542 / 0.238 | 0.99× | 34.94 / 34.86 / 61.69 / 71.14 |
| object_1k / stringify | 644,006 | 0.245 / 0.245 / 0.199 / 0.215 | 1.00× | 35.86 / 35.80 / 61.62 / 70.84 |
| records_array_16k / parse | 6,599 | 23.820 / 24.159 / 40.429 / 33.996 | 0.99× | 64.95 / 64.94 / 65.70 / 70.44 |
| records_array_16k / stringify | 9,986 | 24.208 / 23.953 / 13.215 / 23.266 | 1.01× | 36.17 / 36.09 / 61.69 / 70.98 |
| records_array_1m / parse | 96 | 1,551.479 / 1,572.531 / 2,747.021 / 2,132.438 | 0.99× | 62.19 / 62.12 / 93.45 / 79.11 |
| records_array_1m / stringify | 177 | 1,540.853 / 1,542.582 / 844.746 / 969.944 | 1.00× | 60.50 / 60.50 / 109.08 / 100.31 |
| records_object_1m / parse | 71 | 4,014.225 / 3,997.592 / 2,729.070 / 2,137.183 | 1.00× | 80.02 / 79.94 / 92.72 / 75.95 |
| records_object_1m / stringify | 177 | 1,543.655 / 1,544.102 / 847.011 / 970.412 | 1.00× | 60.48 / 60.47 / 109.11 / 100.31 |
| records_array_8m / parse | 10 | 13,377.600 / 13,567.200 / 34,493.000 / 20,787.200 | 0.99× | 99.56 / 99.53 / 244.11 / 130.97 |
| records_array_8m / stringify | 20 | 13,432.800 / 13,435.200 / 6,810.500 / 8,573.900 | 1.00× | 196.03 / 196.05 / 186.03 / 189.95 |
| records_object_8m / parse | 8 | 31,408.125 / 31,185.500 / 29,652.625 / 21,732.375 | 1.01× | 231.75 / 231.69 / 225.53 / 111.69 |
| records_object_8m / stringify | 20 | 13,439.600 / 13,443.050 / 6,854.450 / 8,446.950 | 1.00× | 196.05 / 196.02 / 185.92 / 169.62 |
| records_array_20m / parse | 3 | 77,894.000 / 77,441.333 / 95,201.000 / 56,659.333 | 1.01× | 315.78 / 315.67 / 325.98 / 188.03 |
| records_array_20m / stringify | 8 | 86,068.000 / 85,923.000 / 17,325.500 / 20,846.500 | 1.00× | 171.38 / 171.31 / 392.17 / 304.80 |
| records_object_20m / parse | 3 | 78,061.333 / 77,380.667 / 94,114.000 / 56,998.000 | 1.01× | 315.78 / 315.67 / 325.94 / 187.95 |
| records_object_20m / stringify | 8 | 85,868.375 / 86,049.000 / 17,356.000 / 20,944.250 | 1.00× | 171.36 / 171.30 / 392.12 / 304.89 |
| numbers_1m / parse | 91 | 1,738.220 / 1,733.165 / 3,122.714 / 3,198.945 | 1.00× | 64.97 / 64.97 / 86.22 / 67.20 |
| numbers_1m / stringify | 96 | 1,604.688 / 1,609.396 / 1,984.958 / 2,953.979 | 1.00× | 61.38 / 61.34 / 92.28 / 72.25 |
| long_string_1m / parse | 1,231 | 204.817 / 204.090 / 369.173 / 68.090 | 1.00× | 60.81 / 60.80 / 156.80 / 160.38 |
| long_string_1m / stringify | 1,143 | 172.265 / 175.685 / 105.634 / 97.344 | 0.98× | 62.02 / 62.00 / 157.89 / 151.58 |
| escaped_1m / parse | 82 | 2,039.085 / 2,040.366 / 1,733.707 / 2,085.829 | 1.00× | 58.22 / 58.20 / 73.89 / 64.19 |
| escaped_1m / stringify | 169 | 954.183 / 966.089 / 1,892.822 / 2,083.568 | 0.99× | 57.23 / 57.20 / 124.62 / 74.77 |
| unicode_1m / parse | 1,537 | 257.949 / 257.560 / 437.824 / 60.118 | 1.00× | 55.55 / 55.53 / 166.62 / 159.77 |
| unicode_1m / stringify | 1,408 | 138.757 / 139.254 / 412.523 / 451.016 | 1.00× | 57.25 / 57.23 / 157.64 / 126.25 |
| wide_1m / parse | 36 | 20,954.750 / 20,902.000 / 4,951.833 / 4,167.389 | 1.00× | 176.70 / 176.69 / 110.62 / 86.14 |
| wide_1m / stringify | 211 | 1,695.464 / 1,692.801 / 6,336.389 / 666.313 | 1.00× | 70.94 / 70.92 / 116.39 / 83.38 |
| heterogeneous_1m / parse | 80 | 1,935.200 / 1,960.600 / 3,868.887 / 2,963.100 | 0.99× | 69.45 / 69.42 / 92.12 / 80.67 |
| heterogeneous_1m / stringify | 167 | 4,389.126 / 4,407.695 / 907.838 / 1,060.713 | 1.00× | 59.22 / 59.20 / 110.62 / 101.48 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.11 / 12.06 / 52.59 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.69 / 24.66 / 74.84 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.23 / 12.19 / 52.72 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 24.80 / 24.77 / 71.28 / 47.03 |
| small_record / retain-parse | 1 | 12.12 / 12.08 / 52.77 / 28.28 |
| small_record / retain-parse | 100,000 | 56.78 / 56.78 / 85.45 / 52.69 |
| small_record / retain-stringify | 1 | 12.30 / 12.23 / 52.69 / 28.34 |
| small_record / retain-stringify | 100,000 | 28.00 / 27.97 / 80.30 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.31 / 16.25 / 58.33 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.50 / 41.45 / 88.58 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.98 / 19.91 / 60.56 / 34.45 |
| records_array_1m / retain-stringify | 16 | 33.84 / 33.78 / 76.36 / 47.25 |
| records_object_1m / retain-parse | 1 | 15.78 / 15.73 / 58.39 / 31.69 |
| records_object_1m / retain-parse | 16 | 66.92 / 66.89 / 88.66 / 55.92 |
| records_object_1m / retain-stringify | 1 | 19.97 / 19.92 / 60.48 / 34.44 |
| records_object_1m / retain-stringify | 16 | 33.86 / 33.80 / 76.28 / 47.22 |
| records_array_8m / retain-parse | 1 | 47.97 / 47.88 / 94.19 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.39 / 86.36 / 146.77 / 91.94 |
| records_array_8m / retain-stringify | 1 | 75.59 / 75.56 / 114.08 / 73.45 |
| records_array_8m / retain-stringify | 4 | 94.89 / 94.86 / 143.91 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.75 / 42.73 / 94.31 / 52.31 |
| records_object_8m / retain-parse | 4 | 113.25 / 113.25 / 146.88 / 91.97 |
| records_object_8m / retain-stringify | 1 | 75.59 / 75.56 / 114.19 / 73.45 |
| records_object_8m / retain-stringify | 4 | 94.89 / 94.86 / 143.95 / 94.48 |
| long_string_1m / retain-parse | 1 | 15.20 / 15.20 / 56.56 / 30.56 |
| long_string_1m / retain-parse | 32 | 56.73 / 56.80 / 89.08 / 61.61 |
| long_string_1m / retain-stringify | 1 | 18.66 / 18.64 / 60.58 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.72 / 58.75 / 92.30 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.41 / 14.38 / 57.59 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.59 / 44.58 / 86.30 / 58.97 |
| unicode_1m / retain-stringify | 1 | 16.64 / 16.61 / 60.89 / 34.11 |
| unicode_1m / retain-stringify | 32 | 46.59 / 46.56 / 89.14 / 62.19 |
| wide_1m / retain-parse | 1 | 23.98 / 23.94 / 65.58 / 36.30 |
| wide_1m / retain-parse | 16 | 71.16 / 71.11 / 112.45 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.06 / 30.02 / 68.55 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.33 / 45.28 / 86.48 / 53.45 |

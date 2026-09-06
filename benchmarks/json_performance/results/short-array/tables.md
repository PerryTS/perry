Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.013 / 0.014 / 0.027 / 0.019 | 0.95× | 13.47 / 13.59 / 57.59 / 35.67 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.56 / 13.73 / 59.53 / 129.83 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 0.96× | 13.45 / 13.59 / 57.53 / 36.05 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.55 / 13.73 / 59.55 / 129.77 |
| empty_object / parse | 2,000,000 | 0.056 / 0.057 / 0.045 / 0.024 | 0.98× | 34.66 / 34.84 / 59.42 / 69.03 |
| empty_object / stringify | 2,000,000 | 0.048 / 0.047 / 0.032 / 0.030 | 1.01× | 13.62 / 13.88 / 59.53 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.120 / 0.120 / 0.081 / 0.045 | 1.00× | 34.69 / 34.88 / 59.48 / 69.08 |
| tiny_object / stringify | 2,000,000 | 0.091 / 0.091 / 0.037 / 0.040 | 1.00× | 34.69 / 34.92 / 59.58 / 129.75 |
| small_record / parse | 585,269 | 0.678 / 0.674 / 0.339 / 0.253 | 1.01× | 34.83 / 34.62 / 59.61 / 79.86 |
| small_record / stringify | 1,259,620 | 0.281 / 0.284 / 0.109 / 0.121 | 0.99× | 35.80 / 36.08 / 59.66 / 252.20 |
| object_1k / parse | 547,445 | 0.590 / 0.589 / 0.540 / 0.237 | 1.00× | 34.91 / 35.09 / 61.67 / 71.12 |
| object_1k / stringify | 650,993 | 0.235 / 0.235 / 0.199 / 0.215 | 1.00× | 35.81 / 36.03 / 61.67 / 70.86 |
| records_array_16k / parse | 6,697 | 24.019 / 23.789 / 39.136 / 33.964 | 1.01× | 64.89 / 64.98 / 65.78 / 70.42 |
| records_array_16k / stringify | 10,126 | 23.406 / 23.262 / 13.277 / 23.213 | 1.01× | 36.08 / 36.31 / 61.77 / 71.00 |
| records_array_1m / parse | 95 | 1,560.411 / 1,554.579 / 2,882.979 / 2,142.442 | 1.00× | 62.09 / 62.16 / 92.95 / 79.12 |
| records_array_1m / stringify | 177 | 1,494.627 / 1,490.271 / 849.571 / 969.198 | 1.00× | 60.39 / 59.45 / 109.20 / 100.31 |
| records_object_1m / parse | 71 | 4,045.239 / 3,988.676 / 2,851.873 / 2,137.197 | 1.01× | 79.98 / 60.28 / 92.81 / 75.94 |
| records_object_1m / stringify | 176 | 1,501.682 / 1,497.960 / 844.415 / 972.420 | 1.00× | 60.39 / 59.45 / 108.34 / 100.28 |
| records_array_8m / parse | 10 | 13,466.300 / 13,366.900 / 34,031.000 / 20,845.900 | 1.01× | 99.52 / 99.56 / 244.09 / 131.12 |
| records_array_8m / stringify | 20 | 13,103.950 / 13,006.350 / 6,813.900 / 8,507.850 | 1.01× | 195.91 / 193.86 / 186.06 / 189.97 |
| records_object_8m / parse | 8 | 31,536.000 / 30,940.125 / 29,214.000 / 21,769.250 | 1.02× | 231.70 / 166.86 / 225.44 / 111.44 |
| records_object_8m / stringify | 20 | 13,097.700 / 12,972.100 / 6,814.750 / 8,435.000 | 1.01× | 195.94 / 193.88 / 186.05 / 169.91 |
| records_array_20m / parse | 3 | 78,185.000 / 76,127.000 / 94,273.000 / 56,713.667 | 1.03× | 315.73 / 223.97 / 325.69 / 187.86 |
| records_array_20m / stringify | 8 | 84,999.500 / 80,218.500 / 17,383.250 / 21,036.750 | 1.06× | 171.23 / 158.84 / 392.19 / 304.83 |
| records_object_20m / parse | 3 | 78,199.000 / 76,178.333 / 93,859.667 / 56,862.667 | 1.03× | 315.73 / 223.97 / 325.86 / 187.97 |
| records_object_20m / stringify | 8 | 84,897.250 / 80,409.375 / 17,397.750 / 21,074.125 | 1.06× | 171.23 / 158.88 / 392.17 / 304.88 |
| numbers_1m / parse | 91 | 1,715.220 / 1,713.571 / 3,121.374 / 3,205.374 | 1.00× | 64.91 / 65.00 / 86.17 / 67.19 |
| numbers_1m / stringify | 96 | 1,605.240 / 1,609.573 / 1,980.417 / 2,952.969 | 1.00× | 61.27 / 61.44 / 92.22 / 72.22 |
| long_string_1m / parse | 1,334 | 203.286 / 203.833 / 368.833 / 67.705 | 1.00× | 60.80 / 60.86 / 160.97 / 166.38 |
| long_string_1m / stringify | 1,098 | 175.403 / 173.281 / 109.082 / 97.828 | 1.01× | 61.95 / 62.06 / 155.86 / 123.61 |
| escaped_1m / parse | 82 | 2,039.305 / 2,040.939 / 1,733.305 / 2,083.793 | 1.00× | 58.17 / 58.23 / 73.88 / 64.20 |
| escaped_1m / stringify | 168 | 964.893 / 952.833 / 1,894.661 / 2,082.530 | 1.01× | 57.16 / 57.27 / 124.62 / 74.73 |
| unicode_1m / parse | 1,470 | 258.181 / 257.056 / 438.080 / 59.267 | 1.00× | 55.50 / 55.53 / 164.92 / 123.17 |
| unicode_1m / stringify | 1,413 | 138.585 / 137.989 / 412.653 / 451.524 | 1.00× | 57.19 / 57.30 / 157.67 / 157.08 |
| wide_1m / parse | 36 | 20,965.306 / 20,897.500 / 4,935.028 / 4,173.750 | 1.00× | 176.69 / 176.78 / 110.52 / 86.11 |
| wide_1m / stringify | 216 | 1,690.648 / 1,690.556 / 6,355.903 / 665.755 | 1.00× | 70.88 / 71.03 / 116.33 / 84.58 |
| heterogeneous_1m / parse | 79 | 1,942.025 / 1,933.063 / 3,947.013 / 2,972.456 | 1.00× | 69.38 / 69.45 / 92.09 / 80.73 |
| heterogeneous_1m / stringify | 167 | 4,363.246 / 4,325.455 / 905.796 / 1,061.114 | 1.01× | 59.12 / 59.30 / 110.66 / 100.53 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.20 / 12.44 / 52.62 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.77 / 25.02 / 74.86 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.31 / 12.59 / 52.78 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 24.86 / 25.17 / 71.22 / 47.03 |
| small_record / retain-parse | 1 | 12.20 / 12.47 / 52.67 / 28.34 |
| small_record / retain-parse | 100,000 | 56.81 / 45.09 / 85.41 / 52.69 |
| small_record / retain-stringify | 1 | 12.34 / 12.66 / 52.73 / 28.31 |
| small_record / retain-stringify | 100,000 | 28.06 / 28.38 / 80.30 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.33 / 16.61 / 58.38 / 31.69 |
| records_array_1m / retain-parse | 16 | 41.53 / 41.81 / 88.62 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.98 / 19.52 / 60.48 / 34.44 |
| records_array_1m / retain-stringify | 16 | 33.88 / 33.42 / 76.36 / 47.25 |
| records_object_1m / retain-parse | 1 | 15.86 / 15.31 / 58.41 / 31.66 |
| records_object_1m / retain-parse | 16 | 66.92 / 56.73 / 88.75 / 55.91 |
| records_object_1m / retain-stringify | 1 | 19.97 / 19.53 / 60.56 / 34.44 |
| records_object_1m / retain-stringify | 16 | 33.86 / 33.44 / 76.34 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.00 / 48.25 / 94.22 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.42 / 86.48 / 146.94 / 91.84 |
| records_array_8m / retain-stringify | 1 | 75.55 / 73.08 / 114.12 / 73.45 |
| records_array_8m / retain-stringify | 4 | 94.84 / 91.81 / 143.97 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.81 / 36.48 / 94.28 / 52.31 |
| records_object_8m / retain-parse | 4 | 113.27 / 97.69 / 146.95 / 91.83 |
| records_object_8m / retain-stringify | 1 | 75.56 / 73.09 / 114.12 / 73.45 |
| records_object_8m / retain-stringify | 4 | 94.86 / 91.83 / 143.94 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.31 / 15.55 / 56.44 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.86 / 56.92 / 89.09 / 61.66 |
| long_string_1m / retain-stringify | 1 | 18.75 / 19.05 / 60.56 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.78 / 58.91 / 92.23 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.48 / 14.73 / 57.58 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.69 / 44.94 / 86.30 / 58.95 |
| unicode_1m / retain-stringify | 1 | 16.72 / 16.98 / 60.81 / 34.09 |
| unicode_1m / retain-stringify | 32 | 46.66 / 46.95 / 89.12 / 61.83 |
| wide_1m / retain-parse | 1 | 24.08 / 24.31 / 65.61 / 36.31 |
| wide_1m / retain-parse | 16 | 71.25 / 71.50 / 112.48 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.20 / 30.45 / 68.61 / 39.25 |
| wide_1m / retain-stringify | 16 | 45.36 / 45.72 / 86.50 / 53.45 |

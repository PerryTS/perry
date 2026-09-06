Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.44 / 13.44 / 57.59 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.50 / 13.52 / 59.55 / 129.77 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.44 / 13.45 / 57.61 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.50 / 13.52 / 59.52 / 129.75 |
| empty_object / parse | 2,000,000 | 0.059 / 0.057 / 0.045 / 0.024 | 1.04× | 34.72 / 34.64 / 59.53 / 69.03 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.64 / 13.64 / 59.50 / 129.77 |
| tiny_object / parse | 2,000,000 | 0.360 / 0.351 / 0.082 / 0.045 | 1.02× | 34.50 / 34.44 / 59.50 / 69.05 |
| tiny_object / stringify | 2,000,000 | 0.095 / 0.094 / 0.037 / 0.040 | 1.01× | 34.72 / 34.62 / 59.66 / 129.77 |
| small_record / parse | 582,429 | 0.676 / 0.666 / 0.361 / 0.252 | 1.01× | 34.86 / 34.80 / 59.53 / 79.88 |
| small_record / stringify | 1,269,841 | 0.321 / 0.285 / 0.109 / 0.121 | 1.13× | 35.86 / 35.78 / 59.67 / 253.94 |
| object_1k / parse | 547,487 | 0.592 / 0.584 / 0.540 / 0.237 | 1.01× | 34.94 / 34.86 / 61.69 / 71.09 |
| object_1k / stringify | 638,751 | 0.245 / 0.241 / 0.199 / 0.215 | 1.02× | 35.84 / 35.78 / 61.69 / 70.84 |
| records_array_16k / parse | 6,617 | 23.861 / 23.804 / 40.600 / 33.919 | 1.00× | 64.95 / 64.84 / 65.64 / 70.45 |
| records_array_16k / stringify | 9,876 | 23.920 / 23.922 / 13.247 / 23.317 | 1.00× | 36.25 / 36.14 / 61.77 / 71.00 |
| records_array_1m / parse | 96 | 1,553.042 / 1,552.594 / 2,668.240 / 2,136.042 | 1.00× | 62.17 / 62.03 / 93.59 / 79.14 |
| records_array_1m / stringify | 174 | 1,545.937 / 1,543.833 / 845.799 / 971.218 | 1.00× | 60.70 / 60.55 / 106.61 / 100.34 |
| records_object_1m / parse | 70 | 4,024.929 / 4,000.971 / 2,881.257 / 2,135.671 | 1.01× | 80.30 / 80.23 / 92.69 / 75.92 |
| records_object_1m / stringify | 176 | 1,547.574 / 1,547.358 / 844.369 / 971.068 | 1.00× | 60.70 / 60.55 / 108.23 / 100.28 |
| records_array_8m / parse | 10 | 13,426.200 / 13,425.000 / 33,902.000 / 20,774.200 | 1.00× | 99.55 / 99.44 / 244.06 / 131.33 |
| records_array_8m / stringify | 21 | 13,335.095 / 13,352.476 / 6,794.619 / 8,461.381 | 1.00× | 195.98 / 195.88 / 185.97 / 176.55 |
| records_object_8m / parse | 8 | 31,371.125 / 31,267.375 / 29,759.625 / 21,659.250 | 1.00× | 231.66 / 231.59 / 225.59 / 111.25 |
| records_object_8m / stringify | 20 | 13,408.850 / 13,455.900 / 6,832.150 / 8,366.900 | 1.00× | 195.98 / 195.88 / 186.06 / 169.75 |
| records_array_20m / parse | 3 | 77,652.667 / 76,823.667 / 98,516.667 / 56,760.000 | 1.01× | 315.64 / 315.56 / 325.97 / 188.16 |
| records_array_20m / stringify | 8 | 85,509.000 / 85,729.125 / 17,303.500 / 21,099.375 | 1.00× | 176.16 / 178.31 / 392.14 / 304.73 |
| records_object_20m / parse | 3 | 77,989.000 / 77,447.667 / 95,562.333 / 56,711.000 | 1.01× | 315.78 / 315.67 / 326.06 / 188.16 |
| records_object_20m / stringify | 8 | 85,538.000 / 86,169.250 / 17,337.250 / 21,053.500 | 0.99× | 171.36 / 171.19 / 392.14 / 304.73 |
| numbers_1m / parse | 91 | 1,712.989 / 1,736.022 / 3,122.813 / 3,211.407 | 0.99× | 64.98 / 64.86 / 86.33 / 67.19 |
| numbers_1m / stringify | 96 | 1,602.906 / 1,607.594 / 1,985.167 / 2,951.969 | 1.00× | 61.36 / 61.23 / 92.23 / 72.23 |
| long_string_1m / parse | 1,284 | 204.406 / 205.415 / 368.988 / 67.861 | 1.00× | 60.84 / 60.72 / 158.86 / 160.41 |
| long_string_1m / stringify | 1,184 | 174.512 / 174.557 / 106.535 / 97.698 | 1.00× | 62.02 / 61.88 / 158.97 / 159.58 |
| escaped_1m / parse | 82 | 2,040.037 / 2,040.134 / 1,734.622 / 2,084.366 | 1.00× | 58.22 / 58.09 / 73.92 / 64.17 |
| escaped_1m / stringify | 169 | 952.929 / 965.615 / 1,900.361 / 2,088.237 | 0.99× | 57.22 / 57.09 / 124.70 / 74.80 |
| unicode_1m / parse | 1,514 | 256.782 / 258.236 / 437.724 / 60.258 | 0.99× | 55.55 / 55.38 / 166.80 / 151.73 |
| unicode_1m / stringify | 1,405 | 138.898 / 139.312 / 413.033 / 451.201 | 1.00× | 57.25 / 57.11 / 157.61 / 126.28 |
| wide_1m / parse | 36 | 20,952.694 / 20,898.861 / 4,953.333 / 4,169.667 | 1.00× | 176.70 / 176.62 / 110.52 / 86.14 |
| wide_1m / stringify | 214 | 1,691.808 / 1,711.771 / 6,362.000 / 666.131 | 0.99× | 70.94 / 70.83 / 116.38 / 84.56 |
| heterogeneous_1m / parse | 80 | 1,933.287 / 1,933.688 / 3,874.963 / 2,984.162 | 1.00× | 69.39 / 69.25 / 92.08 / 80.73 |
| heterogeneous_1m / stringify | 169 | 4,394.402 / 4,400.414 / 905.728 / 1,060.077 | 1.00× | 59.22 / 59.08 / 110.45 / 101.39 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.11 / 12.19 / 52.59 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.69 / 24.80 / 74.81 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.23 / 12.31 / 52.70 / 28.31 |
| tiny_object / retain-stringify | 200,000 | 24.80 / 24.89 / 71.25 / 47.05 |
| small_record / retain-parse | 1 | 12.12 / 12.20 / 52.67 / 28.28 |
| small_record / retain-parse | 100,000 | 56.80 / 56.84 / 85.39 / 52.69 |
| small_record / retain-stringify | 1 | 12.28 / 12.36 / 52.75 / 28.34 |
| small_record / retain-stringify | 100,000 | 28.00 / 28.09 / 80.30 / 51.08 |
| records_array_1m / retain-parse | 1 | 16.30 / 16.39 / 58.41 / 31.67 |
| records_array_1m / retain-parse | 16 | 41.50 / 41.58 / 88.73 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.94 / 20.00 / 60.52 / 34.44 |
| records_array_1m / retain-stringify | 16 | 33.84 / 33.89 / 76.30 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.78 / 15.88 / 58.36 / 31.66 |
| records_object_1m / retain-parse | 16 | 66.88 / 66.91 / 88.73 / 55.92 |
| records_object_1m / retain-stringify | 1 | 19.95 / 20.00 / 60.56 / 34.42 |
| records_object_1m / retain-stringify | 16 | 33.86 / 33.91 / 76.33 / 47.23 |
| records_array_8m / retain-parse | 1 | 47.98 / 48.06 / 94.30 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.41 / 86.42 / 146.89 / 91.95 |
| records_array_8m / retain-stringify | 1 | 75.53 / 75.53 / 114.12 / 73.45 |
| records_array_8m / retain-stringify | 4 | 94.83 / 94.83 / 143.78 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.83 / 42.92 / 94.25 / 52.31 |
| records_object_8m / retain-parse | 4 | 113.28 / 113.31 / 146.88 / 92.11 |
| records_object_8m / retain-stringify | 1 | 75.64 / 75.64 / 114.19 / 73.44 |
| records_object_8m / retain-stringify | 4 | 94.94 / 94.94 / 143.73 / 94.48 |
| long_string_1m / retain-parse | 1 | 15.23 / 15.33 / 56.52 / 30.56 |
| long_string_1m / retain-parse | 32 | 56.80 / 56.81 / 89.09 / 61.66 |
| long_string_1m / retain-stringify | 1 | 18.69 / 18.78 / 60.50 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.73 / 58.75 / 92.30 / 63.77 |
| unicode_1m / retain-parse | 1 | 14.41 / 14.50 / 57.66 / 31.28 |
| unicode_1m / retain-parse | 32 | 44.61 / 44.72 / 86.30 / 58.95 |
| unicode_1m / retain-stringify | 1 | 16.66 / 16.72 / 60.78 / 34.05 |
| unicode_1m / retain-stringify | 32 | 46.59 / 46.69 / 89.14 / 61.77 |
| wide_1m / retain-parse | 1 | 23.97 / 24.09 / 65.59 / 36.33 |
| wide_1m / retain-parse | 16 | 71.16 / 71.28 / 112.50 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.06 / 30.16 / 68.53 / 39.25 |
| wide_1m / retain-stringify | 16 | 45.33 / 45.41 / 86.50 / 53.47 |

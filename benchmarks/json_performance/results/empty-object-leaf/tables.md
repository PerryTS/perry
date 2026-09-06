Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.013 / 0.013 / 0.027 / 0.019 | 1.00× | 13.44 / 13.41 / 57.53 / 35.72 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.56 / 13.48 / 59.52 / 129.78 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.44 / 13.39 / 57.58 / 36.03 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.56 / 13.48 / 59.56 / 129.75 |
| empty_object / parse | 2,000,000 | 0.328 / 0.327 / 0.045 / 0.024 | 1.00× | 34.53 / 34.48 / 59.39 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.056 / 0.049 / 0.032 / 0.030 | 1.12× | 13.64 / 13.62 / 59.50 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.366 / 0.366 / 0.082 / 0.045 | 1.00× | 34.53 / 34.48 / 59.48 / 69.03 |
| tiny_object / stringify | 2,000,000 | 0.141 / 0.140 / 0.037 / 0.040 | 1.01× | 34.77 / 34.70 / 59.55 / 129.73 |
| small_record / parse | 578,405 | 0.749 / 0.747 / 0.341 / 0.254 | 1.00× | 34.91 / 34.88 / 59.52 / 79.80 |
| small_record / stringify | 1,290,322 | 0.529 / 0.519 / 0.108 / 0.121 | 1.02× | 35.84 / 35.81 / 59.64 / 257.42 |
| object_1k / parse | 558,311 | 0.592 / 0.591 / 0.545 / 0.237 | 1.00× | 34.95 / 34.91 / 61.70 / 71.11 |
| object_1k / stringify | 641,025 | 0.288 / 0.288 / 0.199 / 0.215 | 1.00× | 35.88 / 35.83 / 61.66 / 70.81 |
| records_array_16k / parse | 6,685 | 23.856 / 23.836 / 40.726 / 33.957 | 1.00× | 64.92 / 64.97 / 65.59 / 70.45 |
| records_array_16k / stringify | 10,027 | 25.221 / 23.974 / 13.213 / 23.204 | 1.05× | 36.17 / 36.12 / 61.78 / 70.97 |
| records_array_1m / parse | 94 | 1,523.223 / 1,519.723 / 2,654.702 / 2,136.904 | 1.00× | 62.20 / 62.23 / 92.77 / 79.12 |
| records_array_1m / stringify | 177 | 1,606.503 / 1,545.328 / 847.927 / 970.599 | 1.04× | 60.41 / 60.50 / 109.23 / 100.27 |
| records_object_1m / parse | 70 | 4,006.543 / 4,026.171 / 2,806.871 / 2,142.129 | 1.00× | 80.03 / 79.98 / 92.81 / 75.92 |
| records_object_1m / stringify | 176 | 1,614.062 / 1,549.472 / 842.790 / 971.881 | 1.04× | 60.42 / 60.47 / 108.34 / 100.28 |
| records_array_8m / parse | 10 | 13,733.500 / 13,680.200 / 34,505.800 / 21,005.200 | 1.00× | 102.64 / 102.69 / 244.25 / 131.33 |
| records_array_8m / stringify | 19 | 14,037.789 / 13,602.842 / 6,850.579 / 8,278.737 | 1.03× | 195.95 / 195.97 / 185.06 / 163.00 |
| records_object_8m / parse | 7 | 31,686.286 / 31,795.571 / 28,629.286 / 20,679.143 | 1.00× | 213.52 / 213.44 / 210.11 / 106.92 |
| records_object_8m / stringify | 20 | 13,954.050 / 13,482.100 / 6,814.500 / 8,453.350 | 1.04× | 195.98 / 195.95 / 186.02 / 169.73 |
| records_array_20m / parse | 3 | 78,639.333 / 78,697.000 / 95,083.000 / 57,179.333 | 1.00× | 315.81 / 315.69 / 326.12 / 188.02 |
| records_array_20m / stringify | 8 | 87,070.750 / 85,875.625 / 17,353.125 / 20,943.750 | 1.01× | 171.30 / 171.23 / 392.19 / 304.83 |
| records_object_20m / parse | 3 | 78,393.667 / 78,617.333 / 93,776.333 / 57,349.333 | 1.00× | 315.81 / 315.70 / 326.09 / 188.03 |
| records_object_20m / stringify | 8 | 87,182.125 / 85,780.250 / 17,386.250 / 20,895.250 | 1.02× | 171.30 / 171.22 / 392.14 / 320.88 |
| numbers_1m / parse | 89 | 1,695.809 / 1,720.135 / 3,128.539 / 3,201.539 | 0.99× | 64.45 / 64.50 / 84.41 / 65.14 |
| numbers_1m / stringify | 96 | 1,606.833 / 1,609.906 / 1,983.458 / 2,951.698 | 1.00× | 61.33 / 61.39 / 92.36 / 72.22 |
| long_string_1m / parse | 1,327 | 205.181 / 205.370 / 369.059 / 67.845 | 1.00× | 60.80 / 60.83 / 160.98 / 164.36 |
| long_string_1m / stringify | 1,165 | 172.822 / 174.804 / 105.981 / 97.092 | 0.99× | 61.98 / 62.03 / 158.02 / 151.52 |
| escaped_1m / parse | 83 | 2,037.012 / 2,056.229 / 1,733.096 / 2,085.169 | 0.99× | 58.22 / 58.22 / 74.09 / 64.94 |
| escaped_1m / stringify | 83 | 1,860.892 / 1,863.566 / 1,856.313 / 2,093.506 | 1.00× | 57.05 / 57.06 / 79.72 / 68.97 |
| unicode_1m / parse | 1,461 | 257.264 / 256.795 / 438.307 / 60.538 | 1.00× | 56.53 / 56.55 / 165.08 / 154.38 |
| unicode_1m / stringify | 1,418 | 137.594 / 136.544 / 413.163 / 452.578 | 1.01× | 57.20 / 57.23 / 157.72 / 162.41 |
| wide_1m / parse | 36 | 20,936.972 / 20,961.056 / 4,962.306 / 4,165.306 | 1.00× | 176.67 / 176.66 / 110.64 / 86.14 |
| wide_1m / stringify | 213 | 2,478.662 / 2,489.357 / 6,362.000 / 666.310 | 1.00× | 70.91 / 70.95 / 116.53 / 83.23 |
| heterogeneous_1m / parse | 77 | 1,921.831 / 1,910.571 / 3,852.896 / 2,968.701 | 1.01× | 68.08 / 68.14 / 92.66 / 80.69 |
| heterogeneous_1m / stringify | 168 | 4,541.958 / 4,538.345 / 911.804 / 1,064.071 | 1.00× | 59.17 / 59.20 / 110.67 / 101.39 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.23 / 12.22 / 52.72 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.83 / 24.83 / 74.66 / 50.17 |
| tiny_object / retain-stringify | 1 | 12.36 / 12.31 / 52.77 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 24.95 / 24.92 / 71.19 / 47.02 |
| small_record / retain-parse | 1 | 12.22 / 12.23 / 52.69 / 28.28 |
| small_record / retain-parse | 100,000 | 56.88 / 56.89 / 85.50 / 52.67 |
| small_record / retain-stringify | 1 | 12.47 / 12.48 / 52.78 / 28.31 |
| small_record / retain-stringify | 100,000 | 31.33 / 31.34 / 80.31 / 51.03 |
| records_array_1m / retain-parse | 1 | 17.86 / 17.86 / 58.30 / 31.61 |
| records_array_1m / retain-parse | 16 | 41.27 / 41.28 / 88.78 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.05 / 20.08 / 60.59 / 34.41 |
| records_array_1m / retain-stringify | 16 | 33.94 / 33.97 / 76.42 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.89 / 15.94 / 58.39 / 31.62 |
| records_object_1m / retain-parse | 16 | 66.95 / 67.02 / 88.69 / 55.91 |
| records_object_1m / retain-stringify | 1 | 20.03 / 20.08 / 60.55 / 34.39 |
| records_object_1m / retain-stringify | 16 | 33.91 / 33.97 / 76.36 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.52 / 48.50 / 94.33 / 52.30 |
| records_array_8m / retain-parse | 4 | 89.14 / 89.28 / 146.80 / 91.97 |
| records_array_8m / retain-stringify | 1 | 75.59 / 75.66 / 114.09 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.89 / 94.95 / 143.86 / 94.48 |
| records_object_8m / retain-parse | 1 | 42.84 / 42.94 / 94.30 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.30 / 113.36 / 146.95 / 91.92 |
| records_object_8m / retain-stringify | 1 | 75.61 / 75.66 / 114.14 / 73.45 |
| records_object_8m / retain-stringify | 4 | 94.91 / 94.95 / 143.92 / 94.50 |
| long_string_1m / retain-parse | 1 | 15.31 / 15.34 / 56.50 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.75 / 56.95 / 89.11 / 61.64 |
| long_string_1m / retain-stringify | 1 | 18.77 / 18.77 / 60.59 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.73 / 58.91 / 92.25 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.55 / 14.52 / 57.56 / 31.22 |
| unicode_1m / retain-parse | 32 | 44.75 / 44.73 / 86.27 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.56 / 16.50 / 60.81 / 34.02 |
| unicode_1m / retain-stringify | 32 | 46.52 / 46.47 / 89.08 / 61.75 |
| wide_1m / retain-parse | 1 | 24.09 / 24.08 / 65.58 / 36.28 |
| wide_1m / retain-parse | 16 | 71.27 / 71.27 / 112.45 / 68.86 |
| wide_1m / retain-stringify | 1 | 30.17 / 30.17 / 68.59 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.44 / 45.44 / 86.52 / 53.45 |

Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.39 / 13.56 / 57.59 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.50 / 13.69 / 59.47 / 129.80 |
| string_a / parse | 2,000,000 | 0.018 / 0.016 / 0.032 / 0.023 | 1.08× | 13.38 / 13.55 / 57.50 / 36.08 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.50 / 13.69 / 59.52 / 129.75 |
| empty_object / parse | 2,000,000 | 0.323 / 0.058 / 0.045 / 0.024 | 5.59× | 34.38 / 34.73 / 59.42 / 68.98 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.66 / 13.81 / 59.58 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.357 / 0.357 / 0.082 / 0.045 | 1.00× | 34.39 / 34.56 / 59.52 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.095 / 0.094 / 0.037 / 0.039 | 1.01× | 34.61 / 34.78 / 59.66 / 129.75 |
| small_record / parse | 586,415 | 0.675 / 0.671 / 0.343 / 0.252 | 1.00× | 34.77 / 34.92 / 59.45 / 79.86 |
| small_record / stringify | 1,257,424 | 0.315 / 0.316 / 0.109 / 0.121 | 1.00× | 35.77 / 35.92 / 59.67 / 251.80 |
| object_1k / parse | 559,613 | 0.594 / 0.589 / 0.542 / 0.237 | 1.01× | 34.83 / 34.95 / 61.67 / 71.12 |
| object_1k / stringify | 643,488 | 0.244 / 0.243 / 0.199 / 0.216 | 1.00× | 35.75 / 35.92 / 61.72 / 70.81 |
| records_array_16k / parse | 6,703 | 23.790 / 23.845 / 39.366 / 33.948 | 1.00× | 64.78 / 64.97 / 65.73 / 70.39 |
| records_array_16k / stringify | 10,014 | 24.472 / 24.024 / 13.269 / 23.240 | 1.02× | 36.06 / 36.22 / 61.72 / 70.98 |
| records_array_1m / parse | 96 | 1,549.812 / 1,550.656 / 2,926.167 / 2,140.875 | 1.00× | 62.00 / 62.14 / 93.61 / 79.12 |
| records_array_1m / stringify | 177 | 1,553.277 / 1,550.277 / 842.356 / 969.119 | 1.00× | 60.33 / 60.53 / 109.11 / 100.28 |
| records_object_1m / parse | 71 | 4,026.606 / 4,004.437 / 2,849.211 / 2,136.085 | 1.01× | 79.91 / 80.06 / 92.88 / 75.91 |
| records_object_1m / stringify | 174 | 1,551.270 / 1,555.063 / 841.397 / 970.460 | 1.00× | 60.33 / 60.52 / 106.58 / 100.28 |
| records_array_8m / parse | 10 | 13,374.900 / 13,362.400 / 33,267.400 / 20,926.500 | 1.00× | 99.38 / 99.55 / 244.12 / 131.64 |
| records_array_8m / stringify | 21 | 13,356.381 / 13,400.619 / 6,771.095 / 8,406.286 | 1.00× | 195.83 / 196.05 / 185.94 / 176.34 |
| records_object_8m / parse | 8 | 31,353.250 / 31,372.125 / 29,769.875 / 21,758.250 | 1.00× | 231.62 / 231.75 / 225.50 / 111.50 |
| records_object_8m / stringify | 20 | 13,454.300 / 13,492.100 / 6,807.700 / 8,422.000 | 1.00× | 195.86 / 196.05 / 185.98 / 169.55 |
| records_array_20m / parse | 3 | 77,513.000 / 77,724.667 / 96,482.667 / 56,673.000 | 1.00× | 315.66 / 315.75 / 325.78 / 187.95 |
| records_array_20m / stringify | 8 | 86,168.000 / 86,187.250 / 17,374.500 / 20,957.500 | 1.00× | 171.17 / 171.31 / 392.09 / 304.78 |
| records_object_20m / parse | 3 | 77,610.667 / 77,647.000 / 95,287.333 / 56,883.333 | 1.00× | 315.64 / 315.73 / 326.16 / 187.67 |
| records_object_20m / stringify | 8 | 85,729.250 / 85,966.625 / 17,325.875 / 20,888.750 | 1.00× | 171.17 / 171.30 / 392.12 / 304.83 |
| numbers_1m / parse | 92 | 1,713.739 / 1,714.196 / 3,124.293 / 3,200.576 | 1.00× | 64.80 / 64.98 / 87.25 / 68.17 |
| numbers_1m / stringify | 96 | 1,608.135 / 1,610.365 / 1,980.010 / 2,946.698 | 1.00× | 61.19 / 61.38 / 92.20 / 72.20 |
| long_string_1m / parse | 1,244 | 204.105 / 203.215 / 369.370 / 67.982 | 1.00× | 60.69 / 60.86 / 157.89 / 156.41 |
| long_string_1m / stringify | 1,114 | 171.432 / 173.090 / 105.978 / 97.395 | 0.99× | 61.86 / 62.03 / 156.95 / 151.53 |
| escaped_1m / parse | 82 | 2,042.329 / 2,040.854 / 1,733.927 / 2,083.354 | 1.00× | 58.06 / 58.23 / 73.84 / 64.19 |
| escaped_1m / stringify | 83 | 1,864.084 / 1,904.892 / 1,861.916 / 2,102.157 | 0.98× | 56.88 / 57.06 / 79.59 / 68.95 |
| unicode_1m / parse | 1,524 | 258.076 / 261.347 / 437.623 / 59.056 | 0.99× | 55.42 / 55.55 / 166.75 / 123.11 |
| unicode_1m / stringify | 1,372 | 139.679 / 140.282 / 412.909 / 450.388 | 1.00× | 57.11 / 57.28 / 156.69 / 125.88 |
| wide_1m / parse | 36 | 20,856.000 / 20,882.056 / 4,941.639 / 4,159.528 | 1.00× | 174.02 / 176.73 / 110.48 / 86.11 |
| wide_1m / stringify | 216 | 1,691.935 / 1,728.102 / 6,370.574 / 666.194 | 0.98× | 70.78 / 71.02 / 116.42 / 84.62 |
| heterogeneous_1m / parse | 80 | 1,934.400 / 1,934.987 / 3,866.425 / 2,977.387 | 1.00× | 69.27 / 69.44 / 92.19 / 80.83 |
| heterogeneous_1m / stringify | 163 | 4,437.828 / 4,441.865 / 904.712 / 1,061.221 | 1.00× | 59.06 / 59.25 / 110.61 / 101.47 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.11 / 12.34 / 52.66 / 28.31 |
| tiny_object / retain-parse | 200,000 | 24.72 / 24.94 / 74.84 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.25 / 12.48 / 52.73 / 28.33 |
| tiny_object / retain-stringify | 200,000 | 24.84 / 25.05 / 71.22 / 47.02 |
| small_record / retain-parse | 1 | 12.12 / 12.36 / 52.67 / 28.28 |
| small_record / retain-parse | 100,000 | 56.72 / 56.95 / 85.45 / 52.66 |
| small_record / retain-stringify | 1 | 12.33 / 12.52 / 52.80 / 28.30 |
| small_record / retain-stringify | 100,000 | 28.05 / 28.25 / 80.39 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.28 / 16.55 / 58.44 / 31.62 |
| records_array_1m / retain-parse | 16 | 41.48 / 41.73 / 88.70 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.00 / 20.20 / 60.56 / 34.42 |
| records_array_1m / retain-stringify | 16 | 33.86 / 34.08 / 76.30 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.80 / 16.03 / 58.34 / 31.61 |
| records_object_1m / retain-parse | 16 | 66.86 / 67.11 / 88.64 / 55.88 |
| records_object_1m / retain-stringify | 1 | 19.97 / 20.22 / 60.59 / 34.42 |
| records_object_1m / retain-stringify | 16 | 33.88 / 34.11 / 76.28 / 47.22 |
| records_array_8m / retain-parse | 1 | 47.95 / 48.17 / 94.27 / 52.30 |
| records_array_8m / retain-parse | 4 | 86.33 / 86.50 / 146.83 / 91.92 |
| records_array_8m / retain-stringify | 1 | 75.52 / 75.75 / 114.12 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.81 / 95.05 / 143.94 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.75 / 43.05 / 94.27 / 52.28 |
| records_object_8m / retain-parse | 4 | 113.17 / 113.44 / 146.91 / 91.94 |
| records_object_8m / retain-stringify | 1 | 75.53 / 75.77 / 114.19 / 73.45 |
| records_object_8m / retain-stringify | 4 | 94.83 / 95.06 / 143.94 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.25 / 15.48 / 56.50 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.77 / 56.95 / 89.08 / 61.61 |
| long_string_1m / retain-stringify | 1 | 18.72 / 18.95 / 60.58 / 32.59 |
| long_string_1m / retain-stringify | 32 | 58.69 / 58.91 / 92.25 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.44 / 14.67 / 57.59 / 31.28 |
| unicode_1m / retain-parse | 32 | 44.66 / 44.88 / 86.28 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.70 / 16.92 / 60.83 / 34.05 |
| unicode_1m / retain-stringify | 32 | 46.66 / 46.86 / 89.09 / 61.77 |
| wide_1m / retain-parse | 1 | 24.00 / 24.25 / 65.50 / 36.31 |
| wide_1m / retain-parse | 16 | 71.19 / 71.42 / 112.48 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.08 / 30.36 / 68.58 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.45 / 45.62 / 86.38 / 53.45 |

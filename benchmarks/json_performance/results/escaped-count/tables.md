Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.56 / 13.44 / 57.58 / 35.67 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.67 / 13.50 / 59.56 / 129.83 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.55 / 13.44 / 57.58 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.67 / 13.50 / 59.58 / 129.73 |
| empty_object / parse | 2,000,000 | 0.058 / 0.059 / 0.045 / 0.024 | 0.98× | 34.73 / 34.72 / 59.48 / 69.08 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.81 / 13.62 / 59.58 / 129.77 |
| tiny_object / parse | 2,000,000 | 0.356 / 0.360 / 0.080 / 0.045 | 0.99× | 34.56 / 34.50 / 59.55 / 69.03 |
| tiny_object / stringify | 2,000,000 | 0.094 / 0.095 / 0.037 / 0.040 | 0.99× | 34.78 / 34.72 / 59.59 / 129.77 |
| small_record / parse | 585,842 | 0.672 / 0.675 / 0.341 / 0.252 | 1.00× | 34.92 / 34.88 / 59.53 / 79.84 |
| small_record / stringify | 1,299,168 | 0.316 / 0.324 / 0.108 / 0.121 | 0.98× | 35.92 / 35.86 / 59.64 / 258.97 |
| object_1k / parse | 559,049 | 0.590 / 0.593 / 0.542 / 0.237 | 0.99× | 34.98 / 34.92 / 61.64 / 71.14 |
| object_1k / stringify | 649,409 | 0.243 / 0.243 / 0.199 / 0.215 | 1.00× | 35.92 / 35.86 / 61.66 / 70.83 |
| records_array_16k / parse | 6,623 | 23.826 / 23.811 / 39.345 / 33.960 | 1.00× | 64.97 / 64.95 / 65.77 / 70.42 |
| records_array_16k / stringify | 9,742 | 24.412 / 23.928 / 13.232 / 23.228 | 1.02× | 36.23 / 36.16 / 61.78 / 71.00 |
| records_array_1m / parse | 96 | 1,552.458 / 1,549.990 / 2,837.969 / 2,142.510 | 1.00× | 62.16 / 62.12 / 93.58 / 79.19 |
| records_array_1m / stringify | 174 | 1,549.644 / 1,546.966 / 847.695 / 970.736 | 1.00× | 60.52 / 60.52 / 106.41 / 100.34 |
| records_object_1m / parse | 71 | 4,007.310 / 4,011.563 / 2,786.000 / 2,134.972 | 1.00× | 80.09 / 80.00 / 92.88 / 75.91 |
| records_object_1m / stringify | 178 | 1,548.084 / 1,540.697 / 847.843 / 968.966 | 1.00× | 60.52 / 60.48 / 110.03 / 100.28 |
| records_array_8m / parse | 10 | 13,407.900 / 13,402.900 / 33,849.900 / 20,713.700 | 1.00× | 99.55 / 99.53 / 244.02 / 131.97 |
| records_array_8m / stringify | 20 | 13,502.950 / 13,431.300 / 6,774.200 / 8,507.050 | 1.01× | 196.02 / 196.06 / 186.02 / 170.00 |
| records_object_8m / parse | 8 | 31,466.750 / 31,345.750 / 30,433.000 / 21,697.750 | 1.00× | 231.78 / 231.75 / 225.44 / 111.81 |
| records_object_8m / stringify | 20 | 13,499.200 / 13,423.650 / 6,824.250 / 8,517.150 | 1.01× | 196.05 / 196.05 / 186.00 / 189.83 |
| records_array_20m / parse | 3 | 77,805.333 / 77,847.333 / 95,739.333 / 56,707.000 | 1.00× | 315.80 / 315.73 / 326.16 / 188.16 |
| records_array_20m / stringify | 8 | 86,156.625 / 85,897.250 / 17,322.625 / 20,848.500 | 1.00× | 171.36 / 171.31 / 392.20 / 304.62 |
| records_object_20m / parse | 3 | 77,811.333 / 77,820.000 / 98,168.333 / 56,830.667 | 1.00× | 315.80 / 315.73 / 325.94 / 188.09 |
| records_object_20m / stringify | 8 | 86,317.125 / 85,779.750 / 17,335.875 / 21,039.000 | 1.01× | 171.36 / 171.31 / 392.08 / 304.83 |
| numbers_1m / parse | 90 | 1,744.156 / 1,740.600 / 3,125.622 / 3,201.333 | 1.00× | 64.98 / 64.98 / 85.38 / 66.16 |
| numbers_1m / stringify | 96 | 1,609.792 / 1,602.354 / 1,978.896 / 2,955.188 | 1.00× | 61.39 / 61.36 / 92.23 / 72.23 |
| long_string_1m / parse | 1,281 | 206.240 / 206.238 / 369.228 / 68.467 | 1.00× | 60.84 / 60.83 / 158.92 / 170.34 |
| long_string_1m / stringify | 1,184 | 174.347 / 175.872 / 113.099 / 98.021 | 0.99× | 62.03 / 62.03 / 158.92 / 159.62 |
| escaped_1m / parse | 82 | 2,039.012 / 2,043.049 / 1,733.024 / 2,084.878 | 1.00× | 58.23 / 58.22 / 73.89 / 64.16 |
| escaped_1m / stringify | 169 | 1,901.030 / 952.935 / 1,898.444 / 2,086.521 | 1.99× | 58.06 / 57.22 / 124.59 / 74.75 |
| unicode_1m / parse | 1,462 | 256.750 / 257.794 / 438.278 / 59.222 | 1.00× | 55.58 / 55.55 / 164.98 / 123.14 |
| unicode_1m / stringify | 1,333 | 139.028 / 139.539 / 413.382 / 450.651 | 1.00× | 57.27 / 57.25 / 155.80 / 157.05 |
| wide_1m / parse | 36 | 20,877.472 / 20,974.833 / 4,994.222 / 4,176.472 | 1.00× | 176.72 / 176.70 / 110.55 / 86.12 |
| wide_1m / stringify | 216 | 1,726.602 / 1,691.801 / 6,336.940 / 667.264 | 1.02× | 71.02 / 70.94 / 116.44 / 84.33 |
| heterogeneous_1m / parse | 81 | 1,929.654 / 1,933.235 / 3,924.568 / 2,958.062 | 1.00× | 69.45 / 69.44 / 92.17 / 80.78 |
| heterogeneous_1m / stringify | 167 | 4,434.485 / 4,395.695 / 921.647 / 1,060.886 | 1.01× | 59.25 / 59.22 / 110.53 / 100.94 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.34 / 12.11 / 52.61 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.92 / 24.69 / 74.67 / 50.22 |
| tiny_object / retain-stringify | 1 | 12.47 / 12.23 / 52.73 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 25.05 / 24.80 / 71.25 / 47.02 |
| small_record / retain-parse | 1 | 12.36 / 12.12 / 52.69 / 28.28 |
| small_record / retain-parse | 100,000 | 56.94 / 56.80 / 85.42 / 52.67 |
| small_record / retain-stringify | 1 | 12.52 / 12.28 / 52.77 / 28.31 |
| small_record / retain-stringify | 100,000 | 28.23 / 28.00 / 80.30 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.53 / 16.30 / 58.42 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.73 / 41.50 / 88.88 / 55.91 |
| records_array_1m / retain-stringify | 1 | 20.23 / 19.95 / 60.56 / 34.47 |
| records_array_1m / retain-stringify | 16 | 34.11 / 33.84 / 76.28 / 47.23 |
| records_object_1m / retain-parse | 1 | 16.03 / 15.80 / 58.44 / 31.62 |
| records_object_1m / retain-parse | 16 | 67.11 / 66.92 / 88.61 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.22 / 19.95 / 60.53 / 34.44 |
| records_object_1m / retain-stringify | 16 | 34.11 / 33.86 / 76.47 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.20 / 47.94 / 94.28 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.53 / 86.36 / 146.94 / 91.94 |
| records_array_8m / retain-stringify | 1 | 75.75 / 75.59 / 114.14 / 73.45 |
| records_array_8m / retain-stringify | 4 | 95.05 / 94.89 / 143.66 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.98 / 42.78 / 94.25 / 52.31 |
| records_object_8m / retain-parse | 4 | 113.41 / 113.28 / 146.98 / 91.83 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.59 / 114.25 / 73.45 |
| records_object_8m / retain-stringify | 4 | 95.06 / 94.89 / 144.02 / 94.48 |
| long_string_1m / retain-parse | 1 | 15.45 / 15.25 / 56.50 / 30.59 |
| long_string_1m / retain-parse | 32 | 56.91 / 56.78 / 89.22 / 61.62 |
| long_string_1m / retain-stringify | 1 | 18.89 / 18.67 / 60.52 / 32.64 |
| long_string_1m / retain-stringify | 32 | 58.89 / 58.73 / 92.28 / 63.72 |
| unicode_1m / retain-parse | 1 | 14.67 / 14.42 / 57.64 / 31.30 |
| unicode_1m / retain-parse | 32 | 44.88 / 44.59 / 86.27 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.92 / 16.66 / 60.78 / 34.06 |
| unicode_1m / retain-stringify | 32 | 46.86 / 46.58 / 89.19 / 61.77 |
| wide_1m / retain-parse | 1 | 24.25 / 23.98 / 65.56 / 36.31 |
| wide_1m / retain-parse | 16 | 71.42 / 71.16 / 112.50 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.36 / 30.06 / 68.52 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.62 / 45.33 / 86.44 / 53.44 |

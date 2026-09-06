Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.013 / 0.027 / 0.019 | 1.05× | 13.55 / 13.39 / 57.61 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.67 / 13.48 / 59.50 / 129.75 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.04× | 13.55 / 13.41 / 57.56 / 36.06 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.67 / 13.48 / 59.55 / 129.77 |
| empty_object / parse | 2,000,000 | 0.058 / 0.057 / 0.045 / 0.024 | 1.02× | 34.72 / 34.66 / 59.39 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.81 / 13.59 / 59.55 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.356 / 0.354 / 0.082 / 0.045 | 1.01× | 34.56 / 34.48 / 59.55 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.094 / 0.095 / 0.037 / 0.040 | 0.99× | 34.77 / 34.66 / 59.55 / 129.75 |
| small_record / parse | 582,429 | 0.673 / 0.673 / 0.342 / 0.252 | 1.00× | 34.94 / 34.86 / 59.58 / 79.83 |
| small_record / stringify | 1,293,337 | 0.316 / 0.324 / 0.108 / 0.121 | 0.98× | 35.92 / 35.81 / 59.64 / 257.94 |
| object_1k / parse | 523,407 | 0.589 / 0.590 / 0.544 / 0.239 | 1.00× | 35.00 / 34.92 / 61.62 / 71.06 |
| object_1k / stringify | 635,816 | 0.244 / 0.245 / 0.199 / 0.215 | 1.00× | 35.91 / 35.80 / 61.62 / 70.83 |
| records_array_16k / parse | 6,678 | 23.824 / 23.805 / 39.603 / 33.941 | 1.00× | 64.97 / 64.89 / 65.58 / 70.39 |
| records_array_16k / stringify | 10,028 | 24.418 / 24.307 / 13.246 / 23.266 | 1.00× | 36.22 / 36.06 / 61.72 / 70.98 |
| records_array_1m / parse | 96 | 1,551.646 / 1,551.458 / 2,818.000 / 2,137.219 | 1.00× | 62.17 / 62.06 / 93.45 / 79.12 |
| records_array_1m / stringify | 176 | 1,550.193 / 1,548.034 / 840.307 / 971.142 | 1.00× | 60.52 / 60.39 / 108.22 / 100.31 |
| records_object_1m / parse | 71 | 4,011.099 / 4,006.901 / 2,757.521 / 2,134.338 | 1.00× | 80.08 / 79.98 / 92.72 / 75.91 |
| records_object_1m / stringify | 174 | 1,556.034 / 1,555.466 / 847.747 / 969.489 | 1.00× | 60.52 / 60.36 / 106.47 / 100.25 |
| records_array_8m / parse | 10 | 13,374.200 / 13,455.900 / 34,530.200 / 20,864.100 | 0.99× | 99.58 / 99.47 / 243.95 / 131.67 |
| records_array_8m / stringify | 20 | 13,527.450 / 13,477.200 / 6,799.850 / 8,405.150 | 1.00× | 196.02 / 195.92 / 185.94 / 169.73 |
| records_object_8m / parse | 8 | 31,428.875 / 31,399.625 / 30,725.250 / 21,816.125 | 1.00× | 231.77 / 231.70 / 225.45 / 111.17 |
| records_object_8m / stringify | 20 | 13,504.850 / 13,469.900 / 6,819.550 / 8,497.550 | 1.00× | 196.05 / 195.91 / 185.95 / 189.89 |
| records_array_20m / parse | 3 | 77,798.000 / 77,451.000 / 93,416.000 / 56,702.000 | 1.00× | 315.78 / 315.67 / 326.06 / 188.09 |
| records_array_20m / stringify | 8 | 86,185.750 / 85,911.875 / 17,348.875 / 20,817.750 | 1.00× | 171.23 / 171.17 / 392.16 / 304.75 |
| records_object_20m / parse | 3 | 77,736.000 / 77,803.333 / 97,328.333 / 56,606.333 | 1.00× | 315.80 / 315.69 / 325.84 / 187.97 |
| records_object_20m / stringify | 8 | 86,116.125 / 86,043.375 / 17,323.500 / 20,918.250 | 1.00× | 171.36 / 171.17 / 392.14 / 304.70 |
| numbers_1m / parse | 92 | 1,736.935 / 1,713.967 / 3,126.978 / 3,208.196 | 1.01× | 64.97 / 64.91 / 87.25 / 68.17 |
| numbers_1m / stringify | 96 | 1,604.635 / 1,605.802 / 1,980.521 / 2,953.750 | 1.00× | 61.39 / 61.28 / 92.17 / 72.20 |
| long_string_1m / parse | 1,316 | 205.223 / 204.806 / 368.810 / 67.656 | 1.00× | 60.84 / 60.78 / 159.86 / 162.36 |
| long_string_1m / stringify | 1,159 | 175.114 / 175.341 / 105.373 / 97.298 | 1.00× | 62.03 / 61.94 / 157.88 / 151.56 |
| escaped_1m / parse | 82 | 2,043.951 / 2,040.317 / 1,733.927 / 2,087.537 | 1.00× | 58.23 / 58.17 / 73.86 / 64.17 |
| escaped_1m / stringify | 100 | 1,909.890 / 1,569.730 / 1,904.310 / 2,091.690 | 1.22× | 57.09 / 56.16 / 81.95 / 70.92 |
| unicode_1m / parse | 1,440 | 256.999 / 256.259 / 438.206 / 60.439 | 1.00× | 55.56 / 55.50 / 164.03 / 157.92 |
| unicode_1m / stringify | 1,435 | 138.545 / 139.551 / 410.969 / 449.966 | 0.99× | 57.28 / 57.17 / 158.48 / 126.25 |
| wide_1m / parse | 36 | 20,900.333 / 20,897.139 / 4,930.528 / 4,159.333 | 1.00× | 176.72 / 176.70 / 110.52 / 86.14 |
| wide_1m / stringify | 217 | 1,728.493 / 1,690.281 / 6,359.627 / 666.244 | 1.02× | 71.02 / 70.88 / 116.44 / 84.66 |
| heterogeneous_1m / parse | 80 | 1,933.138 / 1,932.750 / 3,930.525 / 2,940.387 | 1.00× | 69.45 / 69.36 / 92.08 / 80.70 |
| heterogeneous_1m / stringify | 167 | 4,438.048 / 4,424.461 / 905.539 / 1,061.174 | 1.00× | 59.25 / 59.11 / 110.55 / 101.33 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.34 / 12.19 / 52.62 / 28.25 |
| tiny_object / retain-parse | 200,000 | 24.94 / 24.77 / 74.72 / 50.17 |
| tiny_object / retain-stringify | 1 | 12.47 / 12.31 / 52.72 / 28.38 |
| tiny_object / retain-stringify | 200,000 | 25.05 / 24.89 / 71.17 / 47.02 |
| small_record / retain-parse | 1 | 12.36 / 12.22 / 52.61 / 28.27 |
| small_record / retain-parse | 100,000 | 56.95 / 56.80 / 85.38 / 52.69 |
| small_record / retain-stringify | 1 | 12.52 / 12.38 / 52.70 / 28.34 |
| small_record / retain-stringify | 100,000 | 28.23 / 28.09 / 80.25 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.53 / 16.39 / 58.38 / 31.59 |
| records_array_1m / retain-parse | 16 | 41.73 / 41.59 / 88.75 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.23 / 20.03 / 60.58 / 34.41 |
| records_array_1m / retain-stringify | 16 | 34.09 / 33.92 / 76.34 / 47.23 |
| records_object_1m / retain-parse | 1 | 16.03 / 15.89 / 58.31 / 31.64 |
| records_object_1m / retain-parse | 16 | 67.09 / 66.94 / 88.77 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.20 / 20.00 / 60.52 / 34.42 |
| records_object_1m / retain-stringify | 16 | 34.11 / 33.91 / 76.25 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.20 / 48.03 / 94.34 / 52.30 |
| records_array_8m / retain-parse | 4 | 86.55 / 86.39 / 146.84 / 91.81 |
| records_array_8m / retain-stringify | 1 | 75.75 / 75.48 / 114.11 / 73.44 |
| records_array_8m / retain-stringify | 4 | 95.05 / 94.80 / 143.67 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.98 / 42.89 / 94.20 / 52.28 |
| records_object_8m / retain-parse | 4 | 113.41 / 113.28 / 146.83 / 91.81 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.48 / 114.17 / 73.45 |
| records_object_8m / retain-stringify | 4 | 95.06 / 94.78 / 143.61 / 94.44 |
| long_string_1m / retain-parse | 1 | 15.45 / 15.34 / 56.52 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.91 / 56.86 / 89.09 / 61.62 |
| long_string_1m / retain-stringify | 1 | 18.89 / 18.77 / 60.53 / 32.59 |
| long_string_1m / retain-stringify | 32 | 58.91 / 58.75 / 92.30 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.67 / 14.53 / 57.67 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.88 / 44.72 / 86.25 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.91 / 16.75 / 60.77 / 34.48 |
| unicode_1m / retain-stringify | 32 | 46.86 / 46.69 / 89.14 / 61.80 |
| wide_1m / retain-parse | 1 | 24.25 / 24.09 / 65.61 / 36.34 |
| wide_1m / retain-parse | 16 | 71.42 / 71.28 / 112.42 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.36 / 30.12 / 68.56 / 39.25 |
| wide_1m / retain-stringify | 16 | 45.62 / 45.39 / 86.42 / 53.44 |

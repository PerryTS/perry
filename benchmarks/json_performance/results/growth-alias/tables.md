Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.59 / 13.41 / 57.53 / 35.67 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.73 / 13.55 / 59.50 / 129.78 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.59 / 13.42 / 57.62 / 36.06 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.73 / 13.56 / 59.52 / 129.77 |
| empty_object / parse | 2,000,000 | 0.057 / 0.057 / 0.045 / 0.024 | 1.00× | 34.84 / 34.66 / 59.41 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.047 / 0.047 / 0.032 / 0.030 | 1.00× | 13.88 / 13.67 / 59.61 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.120 / 0.120 / 0.082 / 0.045 | 1.00× | 34.88 / 34.70 / 59.44 / 69.06 |
| tiny_object / stringify | 2,000,000 | 0.091 / 0.091 / 0.037 / 0.040 | 1.00× | 34.92 / 34.70 / 59.59 / 129.77 |
| small_record / parse | 573,841 | 0.675 / 0.677 / 0.355 / 0.252 | 1.00× | 34.62 / 34.44 / 59.62 / 79.84 |
| small_record / stringify | 1,269,389 | 0.284 / 0.281 / 0.108 / 0.120 | 1.01× | 36.08 / 35.83 / 59.62 / 253.88 |
| object_1k / parse | 565,193 | 0.589 / 0.596 / 0.541 / 0.237 | 0.99× | 35.09 / 34.92 / 61.69 / 71.14 |
| object_1k / stringify | 659,703 | 0.234 / 0.233 / 0.198 / 0.216 | 1.01× | 36.03 / 35.83 / 61.66 / 70.81 |
| records_array_16k / parse | 6,685 | 23.808 / 23.828 / 40.881 / 33.941 | 1.00× | 64.98 / 64.84 / 65.72 / 70.45 |
| records_array_16k / stringify | 10,097 | 23.521 / 23.224 / 13.328 / 23.208 | 1.01× | 36.31 / 36.11 / 61.80 / 71.00 |
| records_array_1m / parse | 95 | 1,555.105 / 1,550.895 / 2,881.316 / 2,135.379 | 1.00× | 62.17 / 62.02 / 92.72 / 79.11 |
| records_array_1m / stringify | 175 | 1,492.663 / 1,494.549 / 843.377 / 968.549 | 1.00× | 59.48 / 59.28 / 107.45 / 100.31 |
| records_object_1m / parse | 71 | 3,982.113 / 3,925.972 / 2,724.085 / 2,180.282 | 1.01× | 60.31 / 60.11 / 92.80 / 75.91 |
| records_object_1m / stringify | 176 | 1,493.665 / 1,494.972 / 848.165 / 970.773 | 1.00× | 59.48 / 59.28 / 108.34 / 100.30 |
| records_array_8m / parse | 10 | 13,415.700 / 13,383.300 / 33,994.400 / 20,924.800 | 1.00× | 99.59 / 99.42 / 244.16 / 131.22 |
| records_array_8m / stringify | 20 | 12,984.600 / 13,043.400 / 6,847.500 / 8,484.750 | 1.00× | 193.81 / 193.66 / 185.91 / 183.16 |
| records_object_8m / parse | 8 | 30,912.000 / 30,570.875 / 29,502.125 / 21,805.375 | 1.01× | 166.89 / 166.67 / 225.39 / 112.11 |
| records_object_8m / stringify | 20 | 13,030.200 / 12,963.850 / 6,840.000 / 8,481.100 | 1.01× | 193.83 / 193.69 / 185.98 / 169.91 |
| records_array_20m / parse | 3 | 76,300.333 / 75,454.333 / 95,347.333 / 56,494.667 | 1.01× | 223.98 / 223.78 / 326.06 / 187.88 |
| records_array_20m / stringify | 8 | 80,142.750 / 80,509.375 / 17,298.875 / 20,864.500 | 1.00× | 158.89 / 158.66 / 392.06 / 304.84 |
| records_object_20m / parse | 3 | 76,202.000 / 75,561.000 / 97,318.000 / 56,692.667 | 1.01× | 223.98 / 223.78 / 326.20 / 187.98 |
| records_object_20m / stringify | 8 | 80,320.500 / 80,293.375 / 17,343.250 / 20,835.000 | 1.00× | 158.92 / 158.69 / 392.09 / 304.89 |
| numbers_1m / parse | 92 | 1,710.772 / 1,711.630 / 3,124.902 / 3,181.804 | 1.00× | 65.00 / 64.86 / 87.23 / 68.19 |
| numbers_1m / stringify | 96 | 1,606.219 / 1,603.729 / 1,975.906 / 2,951.531 | 1.00× | 61.44 / 61.25 / 92.08 / 72.22 |
| long_string_1m / parse | 1,208 | 204.386 / 204.603 / 369.748 / 68.080 | 1.00× | 60.84 / 60.73 / 156.83 / 156.38 |
| long_string_1m / stringify | 1,167 | 172.141 / 173.204 / 105.673 / 96.034 | 0.99× | 62.08 / 61.89 / 157.95 / 123.56 |
| escaped_1m / parse | 82 | 2,039.110 / 2,042.256 / 1,733.256 / 2,082.427 | 1.00× | 58.23 / 58.12 / 73.92 / 64.17 |
| escaped_1m / stringify | 169 | 962.225 / 950.604 / 1,889.533 / 2,083.704 | 1.01× | 57.27 / 57.11 / 124.58 / 74.73 |
| unicode_1m / parse | 1,488 | 257.437 / 257.378 / 438.060 / 60.636 | 1.00× | 55.56 / 55.42 / 165.81 / 154.38 |
| unicode_1m / stringify | 1,356 | 137.620 / 138.415 / 413.657 / 452.819 | 0.99× | 57.30 / 57.12 / 156.66 / 157.52 |
| wide_1m / parse | 36 | 20,901.972 / 20,967.639 / 4,922.583 / 4,158.139 | 1.00× | 176.78 / 176.64 / 110.50 / 86.16 |
| wide_1m / stringify | 216 | 1,689.000 / 1,706.218 / 6,370.269 / 667.671 | 0.99× | 71.03 / 70.86 / 116.48 / 84.59 |
| heterogeneous_1m / parse | 79 | 1,932.089 / 1,932.899 / 3,857.038 / 2,966.228 | 1.00× | 69.47 / 69.31 / 92.17 / 80.84 |
| heterogeneous_1m / stringify | 165 | 4,342.200 / 4,348.745 / 907.752 / 1,060.224 | 1.00× | 59.30 / 59.12 / 110.47 / 101.41 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.45 / 12.39 / 52.55 / 28.30 |
| tiny_object / retain-parse | 200,000 | 25.02 / 24.97 / 74.83 / 50.22 |
| tiny_object / retain-stringify | 1 | 12.61 / 12.48 / 52.70 / 28.36 |
| tiny_object / retain-stringify | 200,000 | 25.17 / 25.08 / 71.23 / 47.03 |
| small_record / retain-parse | 1 | 12.47 / 12.38 / 52.70 / 28.28 |
| small_record / retain-parse | 100,000 | 45.11 / 45.09 / 85.64 / 52.67 |
| small_record / retain-stringify | 1 | 12.66 / 12.52 / 52.73 / 28.38 |
| small_record / retain-stringify | 100,000 | 28.38 / 28.25 / 80.31 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.62 / 16.55 / 58.33 / 31.69 |
| records_array_1m / retain-parse | 16 | 41.81 / 41.73 / 88.75 / 55.89 |
| records_array_1m / retain-stringify | 1 | 19.53 / 19.42 / 60.50 / 34.41 |
| records_array_1m / retain-stringify | 16 | 33.44 / 33.31 / 76.41 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.31 / 15.25 / 58.28 / 31.69 |
| records_object_1m / retain-parse | 16 | 56.73 / 56.75 / 88.69 / 55.92 |
| records_object_1m / retain-stringify | 1 | 19.53 / 19.44 / 60.48 / 34.45 |
| records_object_1m / retain-stringify | 16 | 33.44 / 33.34 / 76.31 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.28 / 48.17 / 94.23 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.53 / 86.55 / 146.92 / 91.80 |
| records_array_8m / retain-stringify | 1 | 73.03 / 73.03 / 114.22 / 73.45 |
| records_array_8m / retain-stringify | 4 | 91.77 / 91.77 / 144.00 / 94.47 |
| records_object_8m / retain-parse | 1 | 36.44 / 36.42 / 94.27 / 52.33 |
| records_object_8m / retain-parse | 4 | 97.66 / 97.70 / 146.92 / 91.91 |
| records_object_8m / retain-stringify | 1 | 73.05 / 73.06 / 114.20 / 73.47 |
| records_object_8m / retain-stringify | 4 | 91.78 / 91.80 / 143.94 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.55 / 15.50 / 56.53 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.92 / 57.00 / 89.14 / 61.66 |
| long_string_1m / retain-stringify | 1 | 19.05 / 18.95 / 60.52 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.91 / 58.94 / 92.25 / 63.77 |
| unicode_1m / retain-parse | 1 | 14.73 / 14.69 / 57.62 / 31.27 |
| unicode_1m / retain-parse | 32 | 44.92 / 44.89 / 86.28 / 58.94 |
| unicode_1m / retain-stringify | 1 | 17.00 / 16.91 / 60.86 / 34.03 |
| unicode_1m / retain-stringify | 32 | 46.95 / 46.86 / 89.12 / 61.78 |
| wide_1m / retain-parse | 1 | 24.33 / 24.25 / 65.59 / 36.36 |
| wide_1m / retain-parse | 16 | 71.50 / 71.44 / 112.44 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.45 / 30.36 / 68.59 / 39.25 |
| wide_1m / retain-stringify | 16 | 45.72 / 45.62 / 86.44 / 53.45 |

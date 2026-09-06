Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.013 / 0.027 / 0.019 | 1.05× | 13.44 / 13.47 / 57.48 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.50 / 13.58 / 59.53 / 129.78 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.04× | 13.44 / 13.47 / 57.52 / 36.12 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.50 / 13.58 / 59.56 / 129.81 |
| empty_object / parse | 2,000,000 | 0.059 / 0.057 / 0.045 / 0.024 | 1.05× | 34.72 / 34.67 / 59.47 / 69.03 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.62 / 13.62 / 59.64 / 129.77 |
| tiny_object / parse | 2,000,000 | 0.360 / 0.361 / 0.081 / 0.046 | 1.00× | 34.50 / 34.50 / 59.47 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.095 / 0.094 / 0.037 / 0.040 | 1.01× | 34.72 / 34.67 / 59.56 / 129.77 |
| small_record / parse | 579,197 | 0.676 / 0.679 / 0.348 / 0.252 | 1.00× | 34.88 / 34.88 / 59.45 / 79.84 |
| small_record / stringify | 1,289,396 | 0.321 / 0.288 / 0.109 / 0.121 | 1.12× | 35.86 / 35.81 / 59.67 / 257.28 |
| object_1k / parse | 555,298 | 0.593 / 0.602 / 0.542 / 0.237 | 0.98× | 34.94 / 34.94 / 61.72 / 71.11 |
| object_1k / stringify | 640,057 | 0.245 / 0.241 / 0.200 / 0.214 | 1.02× | 35.86 / 35.81 / 61.72 / 70.78 |
| records_array_16k / parse | 6,766 | 23.790 / 23.342 / 40.489 / 33.950 | 1.02× | 64.95 / 64.92 / 65.61 / 70.44 |
| records_array_16k / stringify | 9,587 | 24.250 / 23.729 / 13.419 / 23.264 | 1.02× | 36.17 / 36.14 / 61.70 / 71.02 |
| records_array_1m / parse | 97 | 1,551.268 / 1,522.144 / 2,737.691 / 2,134.577 | 1.02× | 62.39 / 62.12 / 94.94 / 79.14 |
| records_array_1m / stringify | 176 | 1,543.886 / 1,522.830 / 844.574 / 969.977 | 1.01× | 60.69 / 60.62 / 108.17 / 100.27 |
| records_object_1m / parse | 71 | 4,010.704 / 3,991.662 / 2,825.028 / 2,135.761 | 1.00× | 80.02 / 80.00 / 92.73 / 75.91 |
| records_object_1m / stringify | 177 | 1,546.169 / 1,526.633 / 844.729 / 970.356 | 1.01× | 60.48 / 60.41 / 109.11 / 100.28 |
| records_array_8m / parse | 10 | 13,418.600 / 13,174.600 / 35,562.300 / 20,990.700 | 1.02× | 99.56 / 99.53 / 244.02 / 130.98 |
| records_array_8m / stringify | 20 | 13,429.250 / 13,289.850 / 6,792.650 / 8,475.700 | 1.01× | 195.98 / 195.86 / 186.02 / 169.62 |
| records_object_8m / parse | 7 | 31,423.143 / 30,960.571 / 29,110.429 / 20,536.429 | 1.01× | 213.48 / 213.45 / 211.16 / 106.80 |
| records_object_8m / stringify | 20 | 13,456.500 / 13,308.550 / 6,787.000 / 8,429.450 | 1.01× | 196.03 / 195.92 / 186.11 / 169.77 |
| records_array_20m / parse | 3 | 77,566.000 / 77,442.000 / 97,607.333 / 56,495.667 | 1.00× | 315.78 / 315.72 / 325.81 / 188.19 |
| records_array_20m / stringify | 8 | 85,597.125 / 85,588.250 / 17,328.875 / 20,778.625 | 1.00× | 171.38 / 171.19 / 392.16 / 304.91 |
| records_object_20m / parse | 3 | 77,877.000 / 77,171.667 / 94,978.333 / 56,738.667 | 1.01× | 315.78 / 315.72 / 326.20 / 187.77 |
| records_object_20m / stringify | 8 | 85,656.250 / 85,505.000 / 17,331.875 / 20,843.875 | 1.00× | 171.36 / 171.17 / 392.12 / 304.89 |
| numbers_1m / parse | 92 | 1,711.554 / 1,726.891 / 3,125.859 / 3,204.141 | 0.99× | 64.98 / 64.95 / 87.23 / 68.19 |
| numbers_1m / stringify | 96 | 1,607.427 / 1,606.948 / 1,970.104 / 2,948.906 | 1.00× | 61.36 / 61.88 / 92.27 / 72.20 |
| long_string_1m / parse | 1,298 | 204.790 / 212.055 / 368.364 / 66.461 | 0.97× | 60.84 / 60.86 / 159.88 / 124.41 |
| long_string_1m / stringify | 1,126 | 172.266 / 173.473 / 106.024 / 97.527 | 0.99× | 62.02 / 61.97 / 156.78 / 153.64 |
| escaped_1m / parse | 83 | 2,039.735 / 1,974.795 / 1,733.096 / 2,083.602 | 1.03× | 58.22 / 58.22 / 73.84 / 64.92 |
| escaped_1m / stringify | 169 | 950.172 / 965.479 / 1,903.355 / 2,087.497 | 0.98× | 57.22 / 57.19 / 124.58 / 74.75 |
| unicode_1m / parse | 1,476 | 256.747 / 263.026 / 438.212 / 60.157 | 0.98× | 55.55 / 55.52 / 164.92 / 156.17 |
| unicode_1m / stringify | 1,349 | 139.535 / 139.110 / 413.746 / 451.111 | 1.00× | 57.25 / 57.20 / 155.75 / 158.91 |
| wide_1m / parse | 36 | 20,961.861 / 20,882.222 / 4,967.139 / 4,164.583 | 1.00× | 176.69 / 176.72 / 110.52 / 86.11 |
| wide_1m / stringify | 213 | 1,695.423 / 1,708.488 / 6,343.451 / 665.944 | 0.99× | 70.94 / 70.92 / 116.39 / 84.67 |
| heterogeneous_1m / parse | 83 | 1,927.988 / 1,880.639 / 3,952.627 / 2,973.024 | 1.03× | 69.39 / 69.34 / 92.69 / 80.72 |
| heterogeneous_1m / stringify | 166 | 4,399.253 / 4,285.313 / 905.506 / 1,062.970 | 1.03× | 59.22 / 59.14 / 110.56 / 100.50 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.11 / 12.34 / 52.62 / 28.34 |
| tiny_object / retain-parse | 200,000 | 24.69 / 24.92 / 74.77 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.23 / 12.42 / 52.70 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 24.78 / 25.00 / 71.23 / 47.03 |
| small_record / retain-parse | 1 | 12.12 / 12.38 / 52.69 / 28.33 |
| small_record / retain-parse | 100,000 | 56.80 / 56.95 / 85.38 / 52.69 |
| small_record / retain-stringify | 1 | 12.28 / 12.48 / 52.77 / 28.31 |
| small_record / retain-stringify | 100,000 | 28.00 / 28.19 / 80.38 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.30 / 16.48 / 58.34 / 31.69 |
| records_array_1m / retain-parse | 16 | 41.50 / 41.69 / 88.59 / 55.89 |
| records_array_1m / retain-stringify | 1 | 19.94 / 20.14 / 60.53 / 34.44 |
| records_array_1m / retain-stringify | 16 | 33.84 / 34.05 / 76.36 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.78 / 16.05 / 58.36 / 31.62 |
| records_object_1m / retain-parse | 16 | 66.91 / 67.08 / 88.67 / 55.89 |
| records_object_1m / retain-stringify | 1 | 19.97 / 20.16 / 60.50 / 34.42 |
| records_object_1m / retain-stringify | 16 | 33.86 / 34.05 / 76.42 / 47.23 |
| records_array_8m / retain-parse | 1 | 47.98 / 48.17 / 94.22 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.42 / 86.58 / 146.88 / 91.91 |
| records_array_8m / retain-stringify | 1 | 75.53 / 75.58 / 114.12 / 73.45 |
| records_array_8m / retain-stringify | 4 | 94.83 / 94.88 / 144.03 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.67 / 43.06 / 94.28 / 52.31 |
| records_object_8m / retain-parse | 4 | 113.25 / 113.44 / 146.83 / 91.81 |
| records_object_8m / retain-stringify | 1 | 75.59 / 75.64 / 114.23 / 73.44 |
| records_object_8m / retain-stringify | 4 | 94.89 / 94.94 / 143.91 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.23 / 15.48 / 56.55 / 30.59 |
| long_string_1m / retain-parse | 32 | 56.78 / 56.98 / 89.16 / 61.66 |
| long_string_1m / retain-stringify | 1 | 18.69 / 18.89 / 60.52 / 32.67 |
| long_string_1m / retain-stringify | 32 | 58.73 / 58.88 / 92.25 / 63.72 |
| unicode_1m / retain-parse | 1 | 14.41 / 14.67 / 57.61 / 31.27 |
| unicode_1m / retain-parse | 32 | 44.61 / 44.88 / 86.27 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.64 / 16.84 / 60.81 / 34.09 |
| unicode_1m / retain-stringify | 32 | 46.59 / 46.80 / 89.08 / 61.81 |
| wide_1m / retain-parse | 1 | 23.98 / 24.27 / 65.59 / 36.31 |
| wide_1m / retain-parse | 16 | 71.16 / 71.45 / 112.41 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.06 / 30.28 / 68.59 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.33 / 45.55 / 86.48 / 53.45 |

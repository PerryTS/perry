Medians of three fresh processes; CPU includes user + system time.

Baseline is nested-records (27eeccf79e); candidate is shape-plans (936c60675). Both use matched runtime/stdlib release settings and the same pinned compiler/application object.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.47 / 13.47 / 57.58 / 35.70 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.56 / 13.56 / 59.55 / 129.77 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 0.99× | 13.48 / 13.47 / 57.56 / 36.05 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.56 / 13.56 / 59.58 / 129.78 |
| empty_object / parse | 2,000,000 | 0.058 / 0.058 / 0.045 / 0.024 | 1.00× | 34.83 / 34.72 / 59.52 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.047 / 0.047 / 0.032 / 0.030 | 1.00× | 13.67 / 13.66 / 59.56 / 129.72 |
| tiny_object / parse | 2,000,000 | 0.121 / 0.120 / 0.080 / 0.045 | 1.00× | 34.84 / 34.73 / 59.47 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.091 / 0.091 / 0.037 / 0.040 | 1.00× | 34.83 / 34.70 / 59.52 / 129.78 |
| small_record / parse | 577,339 | 0.670 / 0.672 / 0.330 / 0.253 | 1.00× | 34.59 / 34.47 / 59.55 / 79.84 |
| small_record / stringify | 1,259,181 | 0.280 / 0.281 / 0.108 / 0.121 | 1.00× | 35.98 / 35.86 / 59.59 / 252.11 |
| object_1k / parse | 549,114 | 0.588 / 0.595 / 0.544 / 0.237 | 0.99× | 35.08 / 34.92 / 61.66 / 71.12 |
| object_1k / stringify | 639,318 | 0.235 / 0.234 / 0.199 / 0.215 | 1.00× | 35.95 / 35.86 / 61.66 / 70.80 |
| records_array_16k / parse | 6,716 | 23.770 / 23.673 / 39.624 / 33.951 | 1.00× | 65.06 / 64.89 / 65.70 / 70.47 |
| records_array_16k / stringify | 9,944 | 23.647 / 23.226 / 13.242 / 23.242 | 1.02× | 36.28 / 36.14 / 61.81 / 71.02 |
| records_array_1m / parse | 96 | 1,555.677 / 1,551.448 / 2,893.427 / 2,144.156 | 1.00× | 62.28 / 62.08 / 93.44 / 79.19 |
| records_array_1m / stringify | 175 | 1,517.960 / 1,494.909 / 844.583 / 969.526 | 1.02× | 59.55 / 59.31 / 107.45 / 100.30 |
| records_object_1m / parse | 70 | 3,921.086 / 3,905.671 / 2,789.414 / 2,191.600 | 1.00× | 58.75 / 58.61 / 92.72 / 75.91 |
| records_object_1m / stringify | 177 | 1,511.638 / 1,493.808 / 848.525 / 970.271 | 1.01× | 59.55 / 59.30 / 109.17 / 100.27 |
| records_array_8m / parse | 10 | 13,439.700 / 13,358.500 / 33,868.000 / 20,821.200 | 1.01× | 99.66 / 99.48 / 244.12 / 131.34 |
| records_array_8m / stringify | 20 | 13,094.300 / 13,007.850 / 6,843.700 / 8,554.350 | 1.01× | 193.80 / 193.70 / 186.19 / 190.08 |
| records_object_8m / parse | 8 | 30,438.625 / 30,175.000 / 30,787.750 / 21,790.625 | 1.01× | 166.88 / 166.69 / 225.45 / 111.11 |
| records_object_8m / stringify | 21 | 13,001.571 / 12,882.857 / 6,804.381 / 8,469.429 | 1.01× | 193.81 / 193.70 / 185.98 / 196.56 |
| records_array_20m / parse | 3 | 75,106.000 / 74,625.000 / 97,020.000 / 56,712.667 | 1.01× | 223.97 / 223.80 / 326.03 / 188.12 |
| records_array_20m / stringify | 8 | 81,319.375 / 81,213.375 / 17,303.125 / 20,833.875 | 1.00× | 158.92 / 158.70 / 392.11 / 304.88 |
| records_object_20m / parse | 3 | 75,175.667 / 74,569.000 / 94,019.667 / 56,559.333 | 1.01× | 223.98 / 223.80 / 326.03 / 187.92 |
| records_object_20m / stringify | 8 | 81,380.625 / 81,344.250 / 17,313.500 / 20,806.750 | 1.00× | 158.95 / 158.70 / 392.22 / 304.80 |
| numbers_1m / parse | 91 | 1,712.495 / 1,740.670 / 3,118.769 / 3,199.505 | 0.98× | 65.09 / 64.91 / 86.39 / 67.20 |
| numbers_1m / stringify | 96 | 1,604.823 / 1,643.010 / 1,986.240 / 2,949.146 | 0.98× | 61.45 / 61.27 / 92.20 / 72.25 |
| long_string_1m / parse | 1,330 | 203.378 / 204.101 / 368.514 / 65.947 | 1.00× | 60.95 / 60.77 / 160.89 / 124.39 |
| long_string_1m / stringify | 1,174 | 171.135 / 172.193 / 105.343 / 97.151 | 0.99× | 62.11 / 61.91 / 158.92 / 153.58 |
| escaped_1m / parse | 82 | 2,039.707 / 2,042.061 / 1,734.220 / 2,083.927 | 1.00× | 58.33 / 58.14 / 73.94 / 64.22 |
| escaped_1m / stringify | 166 | 955.681 / 939.771 / 1,900.777 / 2,085.253 | 1.02× | 57.30 / 57.16 / 124.62 / 74.78 |
| unicode_1m / parse | 1,400 | 257.052 / 257.304 / 438.301 / 59.284 | 1.00× | 55.64 / 55.47 / 163.09 / 123.14 |
| unicode_1m / stringify | 1,427 | 139.987 / 138.708 / 412.333 / 448.629 | 1.01× | 57.31 / 57.17 / 158.50 / 131.19 |
| wide_1m / parse | 36 | 20,865.583 / 20,910.972 / 4,941.500 / 4,159.306 | 1.00× | 176.80 / 176.67 / 110.52 / 86.16 |
| wide_1m / stringify | 216 | 1,689.185 / 1,707.000 / 6,336.648 / 665.250 | 0.99× | 71.05 / 70.84 / 116.45 / 84.39 |
| heterogeneous_1m / parse | 81 | 1,915.901 / 1,909.815 / 3,859.185 / 2,956.012 | 1.00× | 69.56 / 69.38 / 92.55 / 80.80 |
| heterogeneous_1m / stringify | 167 | 1,990.347 / 1,353.539 / 923.820 / 1,060.934 | 1.47× | 59.58 / 59.12 / 110.73 / 101.34 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.14 / 12.17 / 52.62 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.72 / 24.78 / 74.84 / 50.22 |
| tiny_object / retain-stringify | 1 | 12.25 / 12.28 / 52.69 / 28.28 |
| tiny_object / retain-stringify | 200,000 | 24.81 / 24.89 / 71.28 / 47.03 |
| small_record / retain-parse | 1 | 12.17 / 12.19 / 52.69 / 28.28 |
| small_record / retain-parse | 100,000 | 45.05 / 44.97 / 85.44 / 52.69 |
| small_record / retain-stringify | 1 | 12.31 / 12.33 / 52.81 / 28.31 |
| small_record / retain-stringify | 100,000 | 28.03 / 28.08 / 80.31 / 51.05 |
| records_array_1m / retain-parse | 1 | 16.36 / 16.41 / 58.36 / 31.69 |
| records_array_1m / retain-parse | 16 | 41.55 / 41.59 / 88.69 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.22 / 19.16 / 60.52 / 34.42 |
| records_array_1m / retain-stringify | 16 | 33.11 / 33.08 / 76.36 / 47.27 |
| records_object_1m / retain-parse | 1 | 15.05 / 15.09 / 58.38 / 31.61 |
| records_object_1m / retain-parse | 16 | 56.69 / 56.59 / 88.67 / 55.91 |
| records_object_1m / retain-stringify | 1 | 19.19 / 19.16 / 60.55 / 34.44 |
| records_object_1m / retain-stringify | 16 | 33.11 / 33.08 / 76.31 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.02 / 48.03 / 94.27 / 52.33 |
| records_array_8m / retain-parse | 4 | 86.59 / 86.42 / 146.86 / 91.81 |
| records_array_8m / retain-stringify | 1 | 72.94 / 72.89 / 114.14 / 73.45 |
| records_array_8m / retain-stringify | 4 | 91.67 / 91.62 / 143.95 / 94.47 |
| records_object_8m / retain-parse | 1 | 36.17 / 36.27 / 94.36 / 52.31 |
| records_object_8m / retain-parse | 4 | 97.59 / 97.53 / 146.94 / 91.94 |
| records_object_8m / retain-stringify | 1 | 72.95 / 72.89 / 114.19 / 73.47 |
| records_object_8m / retain-stringify | 4 | 91.69 / 91.62 / 143.95 / 94.48 |
| long_string_1m / retain-parse | 1 | 15.30 / 15.33 / 56.61 / 30.58 |
| long_string_1m / retain-parse | 32 | 57.00 / 56.84 / 89.12 / 61.66 |
| long_string_1m / retain-stringify | 1 | 18.73 / 18.77 / 60.53 / 32.67 |
| long_string_1m / retain-stringify | 32 | 58.92 / 58.77 / 92.19 / 63.69 |
| unicode_1m / retain-parse | 1 | 14.44 / 14.55 / 57.67 / 31.27 |
| unicode_1m / retain-parse | 32 | 44.64 / 44.77 / 86.28 / 58.97 |
| unicode_1m / retain-stringify | 1 | 16.67 / 16.73 / 60.83 / 34.48 |
| unicode_1m / retain-stringify | 32 | 46.62 / 46.72 / 89.11 / 61.75 |
| wide_1m / retain-parse | 1 | 24.03 / 24.11 / 65.58 / 36.31 |
| wide_1m / retain-parse | 16 | 71.22 / 71.28 / 112.47 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.12 / 30.12 / 68.48 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.39 / 45.41 / 86.42 / 53.45 |

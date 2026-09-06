Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 0.96× | 13.56 / 13.39 / 57.55 / 35.70 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.67 / 13.48 / 59.59 / 129.84 |
| string_a / parse | 2,000,000 | 0.016 / 0.017 / 0.032 / 0.023 | 0.96× | 13.55 / 13.39 / 57.61 / 36.08 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.67 / 13.48 / 59.53 / 129.78 |
| empty_object / parse | 2,000,000 | 0.058 / 0.059 / 0.045 / 0.024 | 0.98× | 34.73 / 34.70 / 59.42 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.81 / 13.59 / 59.64 / 129.72 |
| tiny_object / parse | 2,000,000 | 0.357 / 0.362 / 0.080 / 0.046 | 0.99× | 34.56 / 34.50 / 59.52 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.094 / 0.093 / 0.037 / 0.040 | 1.01× | 34.77 / 34.70 / 59.58 / 129.73 |
| small_record / parse | 563,203 | 0.673 / 0.682 / 0.336 / 0.254 | 0.99× | 34.94 / 34.88 / 59.47 / 79.81 |
| small_record / stringify | 1,275,694 | 0.315 / 0.320 / 0.109 / 0.121 | 0.99× | 35.92 / 35.84 / 59.59 / 254.94 |
| object_1k / parse | 550,500 | 0.589 / 0.597 / 0.541 / 0.237 | 0.99× | 34.98 / 34.94 / 61.53 / 71.11 |
| object_1k / stringify | 637,619 | 0.244 / 0.243 / 0.199 / 0.215 | 1.01× | 35.92 / 35.84 / 61.58 / 70.78 |
| records_array_16k / parse | 6,697 | 23.861 / 24.961 / 40.331 / 33.973 | 0.96× | 64.97 / 64.97 / 65.83 / 70.42 |
| records_array_16k / stringify | 9,011 | 24.169 / 24.130 / 13.272 / 23.252 | 1.00× | 36.31 / 36.22 / 61.78 / 71.03 |
| records_array_1m / parse | 96 | 1,550.865 / 1,636.448 / 2,875.677 / 2,135.760 | 0.95× | 62.17 / 62.17 / 93.36 / 79.09 |
| records_array_1m / stringify | 175 | 1,551.691 / 1,545.217 / 846.914 / 970.429 | 1.00× | 60.72 / 60.67 / 107.48 / 101.16 |
| records_object_1m / parse | 62 | 4,079.435 / 4,154.435 / 2,850.984 / 2,152.065 | 0.98× | 80.36 / 80.30 / 92.64 / 75.88 |
| records_object_1m / stringify | 174 | 1,557.626 / 1,551.621 / 837.420 / 970.379 | 1.00× | 60.73 / 60.67 / 106.52 / 100.25 |
| records_array_8m / parse | 10 | 13,392.800 / 14,100.300 / 34,743.800 / 20,897.800 | 0.95× | 99.56 / 99.56 / 244.06 / 131.09 |
| records_array_8m / stringify | 20 | 13,486.400 / 13,450.950 / 6,797.300 / 8,436.350 | 1.00× | 195.97 / 195.92 / 185.95 / 169.61 |
| records_object_8m / parse | 8 | 31,399.375 / 32,262.625 / 30,110.375 / 21,491.125 | 0.97× | 231.69 / 231.59 / 225.52 / 110.69 |
| records_object_8m / stringify | 20 | 13,520.950 / 13,470.350 / 6,788.450 / 8,528.650 | 1.00× | 195.98 / 195.92 / 185.89 / 169.72 |
| records_array_20m / parse | 3 | 77,713.667 / 79,407.667 / 96,398.333 / 56,678.000 | 0.98× | 315.66 / 315.56 / 325.75 / 187.94 |
| records_array_20m / stringify | 8 | 85,904.000 / 85,580.125 / 17,343.500 / 20,797.250 | 1.00× | 176.14 / 176.09 / 392.08 / 304.73 |
| records_object_20m / parse | 3 | 77,666.000 / 79,627.667 / 94,536.333 / 56,785.333 | 0.98× | 315.80 / 315.67 / 325.89 / 188.11 |
| records_object_20m / stringify | 8 | 85,981.125 / 85,978.500 / 17,277.375 / 20,880.500 | 1.00× | 171.34 / 171.25 / 392.14 / 304.89 |
| numbers_1m / parse | 92 | 1,711.087 / 1,743.370 / 3,123.870 / 3,206.424 | 0.98× | 64.98 / 64.98 / 87.25 / 68.14 |
| numbers_1m / stringify | 96 | 1,611.792 / 1,609.458 / 1,982.594 / 2,955.719 | 1.00× | 61.38 / 61.34 / 92.25 / 72.20 |
| long_string_1m / parse | 1,345 | 203.366 / 205.604 / 368.616 / 67.497 | 0.99× | 60.86 / 60.84 / 160.91 / 158.34 |
| long_string_1m / stringify | 1,148 | 172.795 / 176.017 / 105.495 / 97.971 | 0.98× | 62.03 / 62.02 / 157.91 / 161.58 |
| escaped_1m / parse | 83 | 2,041.976 / 2,039.241 / 1,734.434 / 2,083.542 | 1.00× | 58.23 / 58.22 / 73.94 / 64.91 |
| escaped_1m / stringify | 82 | 1,905.732 / 1,893.646 / 1,865.854 / 2,101.280 | 1.01× | 57.08 / 56.98 / 79.52 / 68.97 |
| unicode_1m / parse | 1,528 | 257.801 / 259.436 / 437.824 / 60.454 | 0.99× | 55.58 / 55.52 / 166.59 / 166.81 |
| unicode_1m / stringify | 1,444 | 137.919 / 139.902 / 413.020 / 451.606 | 0.99× | 57.28 / 57.23 / 158.67 / 160.67 |
| wide_1m / parse | 36 | 20,909.139 / 21,084.500 / 4,947.611 / 4,165.750 | 0.99× | 176.73 / 176.69 / 110.53 / 86.12 |
| wide_1m / stringify | 215 | 1,724.600 / 1,692.865 / 6,355.349 / 666.126 | 1.02× | 71.02 / 70.92 / 116.48 / 84.42 |
| heterogeneous_1m / parse | 81 | 1,930.062 / 2,040.765 / 3,852.765 / 2,952.469 | 0.95× | 69.39 / 69.38 / 92.25 / 80.70 |
| heterogeneous_1m / stringify | 166 | 4,435.946 / 4,379.524 / 910.289 / 1,059.873 | 1.01× | 59.25 / 59.20 / 110.39 / 100.48 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.34 / 12.11 / 52.59 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.92 / 24.72 / 74.78 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.47 / 12.23 / 52.73 / 28.28 |
| tiny_object / retain-stringify | 200,000 | 25.05 / 24.83 / 71.22 / 47.03 |
| small_record / retain-parse | 1 | 12.36 / 12.12 / 52.72 / 28.27 |
| small_record / retain-parse | 100,000 | 56.95 / 56.80 / 85.48 / 52.69 |
| small_record / retain-stringify | 1 | 12.52 / 12.28 / 52.77 / 28.31 |
| small_record / retain-stringify | 100,000 | 28.25 / 28.03 / 80.28 / 51.05 |
| records_array_1m / retain-parse | 1 | 16.53 / 16.30 / 58.36 / 31.62 |
| records_array_1m / retain-parse | 16 | 41.73 / 41.48 / 88.73 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.20 / 19.95 / 60.50 / 34.41 |
| records_array_1m / retain-stringify | 16 | 34.09 / 33.84 / 76.30 / 47.22 |
| records_object_1m / retain-parse | 1 | 16.03 / 15.81 / 58.39 / 31.62 |
| records_object_1m / retain-parse | 16 | 67.05 / 66.91 / 88.75 / 55.91 |
| records_object_1m / retain-stringify | 1 | 20.22 / 19.97 / 60.45 / 34.41 |
| records_object_1m / retain-stringify | 16 | 34.11 / 33.86 / 76.34 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.22 / 47.97 / 94.22 / 52.28 |
| records_array_8m / retain-parse | 4 | 86.58 / 86.48 / 146.91 / 91.73 |
| records_array_8m / retain-stringify | 1 | 75.69 / 75.50 / 114.11 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.98 / 94.80 / 143.97 / 94.47 |
| records_object_8m / retain-parse | 1 | 43.09 / 42.75 / 94.25 / 52.31 |
| records_object_8m / retain-parse | 4 | 113.44 / 113.28 / 146.62 / 91.91 |
| records_object_8m / retain-stringify | 1 | 75.81 / 75.61 / 114.19 / 73.45 |
| records_object_8m / retain-stringify | 4 | 95.11 / 94.91 / 144.00 / 94.45 |
| long_string_1m / retain-parse | 1 | 15.48 / 15.25 / 56.58 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.95 / 56.84 / 89.20 / 61.64 |
| long_string_1m / retain-stringify | 1 | 18.95 / 18.72 / 60.55 / 32.64 |
| long_string_1m / retain-stringify | 32 | 58.91 / 58.80 / 92.23 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.67 / 14.47 / 57.66 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.88 / 44.66 / 86.31 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.92 / 16.67 / 60.80 / 34.09 |
| unicode_1m / retain-stringify | 32 | 46.86 / 46.62 / 89.14 / 61.75 |
| wide_1m / retain-parse | 1 | 24.23 / 24.03 / 65.52 / 36.28 |
| wide_1m / retain-parse | 16 | 71.42 / 71.22 / 112.42 / 68.91 |
| wide_1m / retain-stringify | 1 | 30.36 / 30.09 / 68.55 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.62 / 45.38 / 86.53 / 53.44 |

Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.261 / 0.260 / 0.029 / 0.020 | 1.00× | 13.53 / 13.56 / 57.30 / 35.72 |
| null / stringify | 2,000,000 | 0.069 / 0.007 / 0.029 / 0.030 | 9.71× | 34.88 / 13.64 / 59.45 / 129.75 |
| string_a / parse | 2,000,000 | 0.265 / 0.263 / 0.033 / 0.023 | 1.00× | 13.56 / 13.56 / 57.30 / 36.06 |
| string_a / stringify | 2,000,000 | 0.078 / 0.010 / 0.031 / 0.030 | 7.54× | 34.86 / 13.67 / 59.34 / 129.78 |
| empty_object / parse | 2,000,000 | 0.339 / 0.339 / 0.047 / 0.026 | 1.00× | 34.64 / 34.58 / 59.28 / 68.98 |
| empty_object / stringify | 2,000,000 | 0.143 / 0.131 / 0.034 / 0.032 | 1.09× | 34.92 / 34.89 / 59.34 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.379 / 0.379 / 0.086 / 0.048 | 1.00× | 34.64 / 34.59 / 59.33 / 69.03 |
| tiny_object / stringify | 2,000,000 | 0.206 / 0.205 / 0.039 / 0.041 | 1.00× | 34.91 / 34.92 / 59.45 / 129.75 |
| small_record / parse | 551,470 | 0.785 / 0.787 / 0.370 / 0.268 | 1.00× | 35.00 / 34.97 / 59.31 / 79.80 |
| small_record / stringify | 1,150,898 | 0.577 / 0.594 / 0.114 / 0.128 | 0.97× | 35.98 / 35.95 / 59.53 / 233.64 |
| object_1k / parse | 547,111 | 0.623 / 0.621 / 0.568 / 0.252 | 1.00× | 35.06 / 35.00 / 61.38 / 71.16 |
| object_1k / stringify | 618,556 | 0.387 / 0.388 / 0.199 / 0.216 | 1.00× | 36.08 / 36.08 / 61.66 / 70.77 |
| records_array_16k / parse | 6,629 | 25.895 / 25.929 / 40.589 / 34.096 | 1.00× | 65.02 / 64.97 / 65.55 / 70.38 |
| records_array_16k / stringify | 9,612 | 36.760 / 34.070 / 13.970 / 24.249 | 1.08× | 36.36 / 36.28 / 61.67 / 71.00 |
| records_array_1m / parse | 89 | 1,678.483 / 1,682.472 / 3,046.640 / 2,291.337 | 1.00× | 62.27 / 62.20 / 91.67 / 79.23 |
| records_array_1m / stringify | 168 | 2,338.119 / 2,188.881 / 906.226 / 1,024.375 | 1.07× | 60.77 / 60.64 / 100.33 / 101.11 |
| records_object_1m / parse | 66 | 4,245.591 / 4,276.727 / 3,122.803 / 2,310.742 | 0.99× | 80.42 / 80.38 / 91.70 / 76.00 |
| records_object_1m / stringify | 170 | 2,336.718 / 2,188.776 / 908.859 / 1,025.365 | 1.07× | 60.77 / 60.69 / 102.03 / 100.30 |
| records_array_8m / parse | 9 | 13,805.556 / 13,877.667 / 35,466.111 / 21,531.444 | 0.99× | 102.77 / 102.69 / 242.39 / 118.56 |
| records_array_8m / stringify | 19 | 19,694.684 / 18,474.158 / 7,273.000 / 8,884.368 | 1.07× | 195.98 / 195.98 / 183.97 / 162.92 |
| records_object_8m / parse | 7 | 33,353.571 / 33,476.857 / 32,248.429 / 24,204.429 | 1.00× | 213.44 / 213.47 / 210.09 / 107.70 |
| records_object_8m / stringify | 19 | 19,629.579 / 18,518.053 / 7,372.947 / 8,873.474 | 1.06× | 195.98 / 196.02 / 184.00 / 163.12 |
| records_array_20m / parse | 3 | 82,643.667 / 82,425.000 / 110,992.333 / 66,267.667 | 1.00× | 315.69 / 315.72 / 324.67 / 187.97 |
| records_array_20m / stringify | 8 | 101,896.250 / 101,044.625 / 18,391.250 / 22,808.625 | 1.01× | 176.16 / 176.16 / 391.06 / 304.92 |
| records_object_20m / parse | 3 | 82,889.000 / 82,934.333 / 110,469.333 / 68,787.000 | 1.00× | 315.69 / 315.72 / 324.72 / 187.61 |
| records_object_20m / stringify | 8 | 102,371.125 / 101,048.625 / 18,344.375 / 22,391.375 | 1.01× | 176.14 / 176.17 / 391.08 / 304.89 |
| numbers_1m / parse | 92 | 1,572.043 / 1,647.402 / 3,225.674 / 3,344.674 | 0.95× | 64.53 / 64.50 / 86.12 / 68.14 |
| numbers_1m / stringify | 75 | 6,663.467 / 2,373.307 / 2,029.733 / 2,959.067 | 2.81× | 61.44 / 61.36 / 80.86 / 70.20 |
| long_string_1m / parse | 1,164 | 204.933 / 204.287 / 370.070 / 67.534 | 1.00× | 60.91 / 60.81 / 153.59 / 124.42 |
| long_string_1m / stringify | 1,081 | 204.138 / 206.450 / 106.421 / 98.346 | 0.99× | 64.02 / 64.00 / 154.66 / 157.52 |
| escaped_1m / parse | 80 | 2,042.588 / 2,040.987 / 1,736.237 / 2,085.613 | 1.00× | 58.30 / 58.20 / 72.83 / 63.39 |
| escaped_1m / stringify | 83 | 1,935.410 / 2,001.530 / 1,931.627 / 2,172.024 | 0.97× | 57.08 / 57.03 / 78.50 / 68.94 |
| unicode_1m / parse | 1,427 | 270.034 / 271.773 / 454.516 / 63.104 | 0.99× | 55.64 / 55.53 / 162.94 / 123.17 |
| unicode_1m / stringify | 635 | 269.748 / 270.850 / 438.513 / 469.206 | 1.00× | 58.34 / 58.30 / 136.42 / 95.47 |
| wide_1m / parse | 35 | 21,362.629 / 21,346.914 / 5,308.371 / 4,323.800 | 1.00× | 176.59 / 176.59 / 109.45 / 84.91 |
| wide_1m / stringify | 208 | 2,594.966 / 2,578.375 / 6,632.087 / 691.072 | 1.01× | 71.00 / 70.97 / 115.39 / 84.39 |
| heterogeneous_1m / parse | 78 | 1,870.974 / 1,877.359 / 3,856.436 / 2,990.038 | 1.00× | 68.06 / 67.97 / 90.94 / 80.67 |
| heterogeneous_1m / stringify | 169 | 4,501.331 / 4,503.089 / 901.467 / 1,060.568 | 1.00× | 59.27 / 59.20 / 109.42 / 101.39 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.33 / 12.28 / 51.41 / 28.25 |
| tiny_object / retain-parse | 200,000 | 24.91 / 24.84 / 74.64 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.58 / 51.55 / 28.31 |
| tiny_object / retain-stringify | 200,000 | 36.41 / 36.39 / 71.09 / 47.03 |
| small_record / retain-parse | 1 | 12.36 / 12.30 / 51.47 / 28.27 |
| small_record / retain-parse | 100,000 | 57.02 / 56.94 / 85.22 / 52.67 |
| small_record / retain-stringify | 1 | 12.61 / 12.62 / 51.61 / 28.34 |
| small_record / retain-stringify | 100,000 | 31.44 / 31.45 / 80.20 / 51.03 |
| records_array_1m / retain-parse | 1 | 17.97 / 17.94 / 57.22 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.36 / 41.36 / 87.61 / 55.88 |
| records_array_1m / retain-stringify | 1 | 20.16 / 20.11 / 59.36 / 34.39 |
| records_array_1m / retain-stringify | 16 | 34.05 / 34.00 / 75.22 / 47.20 |
| records_object_1m / retain-parse | 1 | 15.98 / 15.94 / 57.34 / 31.59 |
| records_object_1m / retain-parse | 16 | 67.09 / 66.95 / 87.58 / 55.88 |
| records_object_1m / retain-stringify | 1 | 20.17 / 20.17 / 59.39 / 34.41 |
| records_object_1m / retain-stringify | 16 | 34.06 / 34.06 / 75.14 / 47.20 |
| records_array_8m / retain-parse | 1 | 48.64 / 48.61 / 93.19 / 52.30 |
| records_array_8m / retain-parse | 4 | 89.33 / 89.22 / 145.84 / 91.69 |
| records_array_8m / retain-stringify | 1 | 75.70 / 75.61 / 113.02 / 73.44 |
| records_array_8m / retain-stringify | 4 | 95.00 / 94.91 / 142.72 / 94.44 |
| records_object_8m / retain-parse | 1 | 43.05 / 42.88 / 93.11 / 52.27 |
| records_object_8m / retain-parse | 4 | 113.42 / 113.41 / 145.77 / 91.83 |
| records_object_8m / retain-stringify | 1 | 75.81 / 75.75 / 112.95 / 73.44 |
| records_object_8m / retain-stringify | 4 | 95.11 / 95.05 / 142.84 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.44 / 15.39 / 55.30 / 30.50 |
| long_string_1m / retain-parse | 32 | 56.98 / 56.84 / 87.92 / 61.62 |
| long_string_1m / retain-stringify | 1 | 21.14 / 21.17 / 59.38 / 32.59 |
| long_string_1m / retain-stringify | 32 | 59.89 / 59.81 / 91.06 / 63.70 |
| unicode_1m / retain-parse | 1 | 14.62 / 14.56 / 56.55 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.84 / 44.78 / 85.12 / 58.92 |
| unicode_1m / retain-stringify | 1 | 18.73 / 18.72 / 59.58 / 34.03 |
| unicode_1m / retain-stringify | 32 | 48.69 / 48.67 / 88.02 / 61.75 |
| wide_1m / retain-parse | 1 | 24.20 / 24.12 / 64.48 / 36.31 |
| wide_1m / retain-parse | 16 | 71.38 / 71.31 / 111.34 / 68.83 |
| wide_1m / retain-stringify | 1 | 30.28 / 30.31 / 67.34 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.55 / 45.58 / 85.22 / 53.42 |

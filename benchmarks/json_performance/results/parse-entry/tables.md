Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.013 / 0.014 / 0.027 / 0.019 | 0.98× | 13.38 / 13.56 / 57.62 / 35.70 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.48 / 13.61 / 59.55 / 129.75 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 0.98× | 13.38 / 13.56 / 57.59 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.48 / 13.62 / 59.56 / 129.78 |
| empty_object / parse | 2,000,000 | 0.320 / 0.321 / 0.045 / 0.024 | 1.00× | 34.48 / 34.56 / 59.45 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.61 / 13.72 / 59.53 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.357 / 0.353 / 0.082 / 0.045 | 1.01× | 34.50 / 34.58 / 59.55 / 69.03 |
| tiny_object / stringify | 2,000,000 | 0.140 / 0.140 / 0.037 / 0.040 | 1.00× | 34.72 / 34.78 / 59.50 / 129.73 |
| small_record / parse | 581,301 | 0.676 / 0.671 / 0.348 / 0.253 | 1.01× | 34.89 / 34.97 / 59.58 / 79.86 |
| small_record / stringify | 1,275,011 | 0.527 / 0.525 / 0.109 / 0.121 | 1.00× | 35.83 / 35.95 / 59.62 / 254.81 |
| object_1k / parse | 522,268 | 0.589 / 0.591 / 0.543 / 0.238 | 1.00× | 34.92 / 35.00 / 61.62 / 71.11 |
| object_1k / stringify | 629,316 | 0.288 / 0.288 / 0.199 / 0.215 | 1.00× | 35.84 / 35.91 / 61.72 / 70.81 |
| records_array_16k / parse | 6,754 | 23.664 / 23.623 / 41.909 / 33.927 | 1.00× | 64.88 / 65.02 / 65.69 / 70.44 |
| records_array_16k / stringify | 10,041 | 24.480 / 24.198 / 13.183 / 23.182 | 1.01× | 36.09 / 36.22 / 61.80 / 70.98 |
| records_array_1m / parse | 95 | 1,506.874 / 1,513.705 / 2,653.589 / 2,135.579 | 1.00× | 62.14 / 66.89 / 92.67 / 79.09 |
| records_array_1m / stringify | 178 | 1,555.466 / 1,559.910 / 847.090 / 971.483 | 1.00× | 60.36 / 60.53 / 110.06 / 100.25 |
| records_object_1m / parse | 71 | 4,029.296 / 4,000.211 / 2,769.690 / 2,140.887 | 1.01× | 80.02 / 80.09 / 92.70 / 75.91 |
| records_object_1m / stringify | 177 | 1,560.412 / 1,562.113 / 849.124 / 969.808 | 1.00× | 60.34 / 60.50 / 109.16 / 100.27 |
| records_array_8m / parse | 10 | 13,545.300 / 13,506.700 / 34,400.900 / 20,916.900 | 1.00× | 102.59 / 88.16 / 244.28 / 131.53 |
| records_array_8m / stringify | 21 | 13,453.143 / 13,454.952 / 6,764.286 / 8,416.190 | 1.00× | 195.86 / 196.06 / 186.03 / 176.11 |
| records_object_8m / parse | 8 | 31,459.500 / 31,375.625 / 29,630.375 / 21,693.375 | 1.00× | 231.70 / 231.78 / 225.45 / 111.61 |
| records_object_8m / stringify | 20 | 13,528.550 / 13,542.950 / 6,798.350 / 8,477.550 | 1.00× | 195.88 / 196.03 / 185.97 / 169.77 |
| records_array_20m / parse | 3 | 78,556.667 / 77,822.000 / 96,363.667 / 56,743.667 | 1.01× | 315.73 / 315.77 / 325.94 / 187.70 |
| records_array_20m / stringify | 8 | 86,424.500 / 86,246.625 / 17,442.250 / 20,978.500 | 1.00× | 171.22 / 171.31 / 392.11 / 304.77 |
| records_object_20m / parse | 3 | 78,012.333 / 77,719.333 / 94,290.667 / 57,367.000 | 1.00× | 315.73 / 315.77 / 325.94 / 188.03 |
| records_object_20m / stringify | 8 | 86,368.875 / 86,185.500 / 17,323.125 / 20,871.125 | 1.00× | 171.20 / 171.30 / 392.19 / 304.75 |
| numbers_1m / parse | 88 | 1,696.727 / 1,723.739 / 3,126.591 / 3,208.364 | 0.98× | 64.42 / 64.92 / 83.45 / 65.17 |
| numbers_1m / stringify | 96 | 1,604.760 / 1,605.062 / 1,979.521 / 2,952.958 | 1.00× | 61.27 / 61.39 / 92.23 / 72.20 |
| long_string_1m / parse | 1,216 | 206.176 / 206.940 / 369.166 / 68.068 | 1.00× | 60.77 / 60.88 / 156.81 / 164.33 |
| long_string_1m / stringify | 1,151 | 172.341 / 172.771 / 107.181 / 97.296 | 1.00× | 61.94 / 63.11 / 157.88 / 151.56 |
| escaped_1m / parse | 82 | 2,039.841 / 2,042.354 / 1,732.902 / 2,084.146 | 1.00× | 58.16 / 58.27 / 73.91 / 64.20 |
| escaped_1m / stringify | 84 | 1,860.083 / 1,906.714 / 1,849.810 / 2,091.500 | 0.98× | 56.94 / 57.05 / 80.52 / 68.92 |
| unicode_1m / parse | 1,431 | 256.447 / 257.897 / 438.187 / 60.564 | 0.99× | 55.50 / 55.56 / 164.03 / 156.16 |
| unicode_1m / stringify | 1,337 | 138.926 / 138.862 / 414.815 / 448.485 | 1.00× | 57.19 / 57.27 / 155.75 / 131.17 |
| wide_1m / parse | 36 | 20,838.889 / 20,897.333 / 5,102.472 / 4,161.528 | 1.00× | 176.64 / 176.77 / 110.52 / 86.09 |
| wide_1m / stringify | 217 | 1,687.770 / 1,690.705 / 6,385.982 / 666.129 | 1.00× | 70.84 / 70.95 / 116.50 / 84.55 |
| heterogeneous_1m / parse | 78 | 1,890.885 / 1,892.167 / 3,871.987 / 2,963.167 | 1.00× | 68.05 / 71.80 / 92.20 / 80.34 |
| heterogeneous_1m / stringify | 168 | 4,593.232 / 4,378.143 / 905.661 / 1,062.952 | 1.05× | 59.09 / 59.25 / 110.53 / 101.39 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.20 / 12.25 / 52.67 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.78 / 24.83 / 74.75 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.33 / 12.36 / 52.70 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 24.89 / 24.91 / 71.17 / 47.02 |
| small_record / retain-parse | 1 | 12.23 / 12.28 / 52.62 / 28.28 |
| small_record / retain-parse | 100,000 | 56.81 / 56.88 / 85.52 / 52.67 |
| small_record / retain-stringify | 1 | 12.48 / 12.53 / 52.78 / 28.28 |
| small_record / retain-stringify | 100,000 | 31.31 / 31.34 / 80.30 / 51.05 |
| records_array_1m / retain-parse | 1 | 17.86 / 17.98 / 58.38 / 31.61 |
| records_array_1m / retain-parse | 16 | 41.25 / 41.39 / 88.67 / 55.88 |
| records_array_1m / retain-stringify | 1 | 20.05 / 20.11 / 60.47 / 34.41 |
| records_array_1m / retain-stringify | 16 | 33.91 / 34.00 / 76.25 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.89 / 15.95 / 58.39 / 31.62 |
| records_object_1m / retain-parse | 16 | 66.94 / 67.05 / 88.72 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.02 / 20.09 / 60.52 / 34.42 |
| records_object_1m / retain-stringify | 16 | 33.92 / 34.00 / 76.33 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.52 / 48.59 / 94.23 / 52.30 |
| records_array_8m / retain-parse | 4 | 89.16 / 87.00 / 146.94 / 91.97 |
| records_array_8m / retain-stringify | 1 | 75.53 / 75.64 / 114.17 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.83 / 94.94 / 143.67 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.84 / 42.95 / 94.36 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.27 / 113.36 / 146.95 / 91.94 |
| records_object_8m / retain-stringify | 1 | 75.53 / 75.64 / 114.25 / 73.44 |
| records_object_8m / retain-stringify | 4 | 94.83 / 94.94 / 143.97 / 94.45 |
| long_string_1m / retain-parse | 1 | 15.34 / 15.38 / 56.53 / 30.56 |
| long_string_1m / retain-parse | 32 | 56.77 / 56.97 / 89.09 / 61.62 |
| long_string_1m / retain-stringify | 1 | 18.80 / 18.83 / 60.56 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.78 / 58.88 / 92.27 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.53 / 14.56 / 57.70 / 31.23 |
| unicode_1m / retain-parse | 32 | 44.73 / 44.75 / 86.34 / 58.95 |
| unicode_1m / retain-stringify | 1 | 16.78 / 16.78 / 60.84 / 34.08 |
| unicode_1m / retain-stringify | 32 | 46.72 / 46.72 / 89.12 / 61.73 |
| wide_1m / retain-parse | 1 | 24.09 / 24.16 / 65.56 / 36.27 |
| wide_1m / retain-parse | 16 | 71.27 / 71.33 / 112.50 / 68.91 |
| wide_1m / retain-stringify | 1 | 30.16 / 30.17 / 68.55 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.42 / 45.44 / 86.48 / 53.45 |

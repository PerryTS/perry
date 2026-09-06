Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.251 / 0.250 / 0.027 / 0.019 | 1.00× | 13.55 / 13.55 / 57.23 / 35.64 |
| null / stringify | 2,000,000 | 0.064 / 0.063 / 0.027 / 0.029 | 1.01× | 34.94 / 34.91 / 59.36 / 129.70 |
| string_a / parse | 2,000,000 | 0.255 / 0.254 / 0.032 / 0.023 | 1.00× | 13.56 / 13.56 / 57.38 / 35.97 |
| string_a / stringify | 2,000,000 | 0.072 / 0.076 / 0.030 / 0.030 | 0.95× | 34.91 / 34.86 / 59.38 / 129.77 |
| empty_object / parse | 2,000,000 | 0.326 / 0.325 / 0.045 / 0.024 | 1.00× | 34.69 / 34.67 / 59.30 / 68.95 |
| empty_object / stringify | 2,000,000 | 0.136 / 0.135 / 0.032 / 0.030 | 1.01× | 34.98 / 34.94 / 59.38 / 129.70 |
| tiny_object / parse | 2,000,000 | 0.365 / 0.364 / 0.081 / 0.045 | 1.00× | 34.69 / 34.67 / 59.31 / 68.98 |
| tiny_object / stringify | 2,000,000 | 0.196 / 0.194 / 0.037 / 0.039 | 1.01× | 34.98 / 34.94 / 59.41 / 129.72 |
| small_record / parse | 560,965 | 0.788 / 0.782 / 0.372 / 0.253 | 1.01× | 35.08 / 35.06 / 59.28 / 79.80 |
| small_record / stringify | 1,219,095 | 0.573 / 0.573 / 0.114 / 0.126 | 1.00× | 36.06 / 36.00 / 59.55 / 245.25 |
| object_1k / parse | 533,373 | 0.592 / 0.592 / 0.545 / 0.239 | 1.00× | 35.11 / 35.09 / 61.39 / 71.03 |
| object_1k / stringify | 596,866 | 0.388 / 0.385 / 0.200 / 0.216 | 1.01× | 36.14 / 36.08 / 61.61 / 70.75 |
| records_array_16k / parse | 6,635 | 23.825 / 23.854 / 40.826 / 33.935 | 1.00× | 65.08 / 65.00 / 65.48 / 70.38 |
| records_array_16k / stringify | 10,112 | 36.391 / 35.976 / 13.821 / 23.517 | 1.01× | 36.44 / 36.38 / 61.67 / 70.97 |
| records_array_1m / parse | 89 | 1,544.045 / 1,549.135 / 3,015.011 / 2,146.427 | 1.00× | 62.33 / 62.34 / 91.73 / 79.14 |
| records_array_1m / stringify | 167 | 2,354.000 / 2,330.066 / 906.737 / 1,021.659 | 1.01× | 61.70 / 61.58 / 99.41 / 100.30 |
| records_object_1m / parse | 67 | 4,286.985 / 4,261.776 / 2,961.119 / 2,283.761 | 1.01× | 80.48 / 80.47 / 91.64 / 75.97 |
| records_object_1m / stringify | 165 | 2,350.273 / 2,350.055 / 906.564 / 1,029.782 | 1.00× | 61.70 / 61.58 / 97.81 / 100.33 |
| records_array_8m / parse | 9 | 14,605.444 / 14,635.889 / 38,212.667 / 24,843.000 | 1.00× | 102.81 / 102.73 / 242.39 / 118.31 |
| records_array_8m / stringify | 19 | 19,747.211 / 19,754.211 / 7,283.789 / 8,707.947 | 1.00× | 196.12 / 196.00 / 183.94 / 163.00 |
| records_object_8m / parse | 7 | 33,114.143 / 33,370.571 / 31,555.714 / 23,351.000 | 0.99× | 213.48 / 213.48 / 210.22 / 107.12 |
| records_object_8m / stringify | 19 | 19,573.789 / 19,541.368 / 7,145.579 / 8,707.158 | 1.00× | 196.12 / 196.00 / 183.95 / 163.00 |
| records_array_20m / parse | 3 | 79,167.333 / 79,438.667 / 95,429.000 / 56,529.000 | 1.00× | 315.73 / 315.73 / 324.86 / 188.17 |
| records_array_20m / stringify | 8 | 97,365.125 / 97,449.250 / 17,385.625 / 20,846.125 | 1.00× | 176.28 / 176.16 / 391.03 / 304.67 |
| records_object_20m / parse | 3 | 78,668.333 / 79,317.333 / 96,226.667 / 56,136.000 | 0.99× | 315.73 / 315.73 / 324.64 / 187.81 |
| records_object_20m / stringify | 8 | 97,521.875 / 97,261.500 / 17,368.250 / 20,849.875 | 1.00× | 176.27 / 176.14 / 391.05 / 304.80 |
| numbers_1m / parse | 96 | 1,575.521 / 1,577.000 / 3,124.115 / 3,212.188 | 1.00× | 64.58 / 64.52 / 89.94 / 68.16 |
| numbers_1m / stringify | 75 | 6,602.120 / 6,620.973 / 1,933.733 / 2,952.040 | 1.00× | 61.55 / 61.45 / 80.66 / 70.20 |
| long_string_1m / parse | 1,353 | 205.477 / 204.586 / 368.349 / 65.873 | 1.00× | 62.98 / 62.92 / 159.81 / 124.34 |
| long_string_1m / stringify | 1,060 | 206.013 / 206.922 / 106.483 / 97.836 | 1.00× | 65.09 / 64.98 / 153.67 / 155.50 |
| escaped_1m / parse | 82 | 2,042.756 / 2,042.829 / 1,733.756 / 2,086.817 | 1.00× | 58.34 / 58.28 / 72.86 / 64.17 |
| escaped_1m / stringify | 83 | 1,859.651 / 1,863.578 / 1,852.060 / 2,097.277 | 1.00× | 57.12 / 57.03 / 78.55 / 68.92 |
| unicode_1m / parse | 1,451 | 984.060 / 982.664 / 438.569 / 60.722 | 1.00× | 56.50 / 56.47 / 163.72 / 163.27 |
| unicode_1m / stringify | 326 | 984.442 / 986.000 / 436.613 / 456.647 | 1.00× | 59.20 / 59.11 / 128.19 / 80.92 |
| wide_1m / parse | 36 | 44,354.139 / 21,043.528 / 4,943.694 / 4,153.750 | 2.11× | 226.95 / 176.81 / 109.33 / 86.11 |
| wide_1m / stringify | 216 | 2,513.958 / 2,476.884 / 6,323.546 / 665.009 | 1.01× | 69.86 / 70.97 / 115.17 / 84.47 |
| heterogeneous_1m / parse | 78 | 1,868.872 / 1,871.372 / 3,951.000 / 2,964.192 | 1.00× | 68.11 / 68.02 / 90.95 / 80.64 |
| heterogeneous_1m / stringify | 167 | 4,526.587 / 4,495.449 / 906.246 / 1,062.431 | 1.01× | 60.14 / 60.03 / 109.33 / 100.48 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.33 / 12.22 / 51.42 / 28.22 |
| tiny_object / retain-parse | 200,000 | 24.86 / 24.78 / 74.67 / 50.17 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.47 / 51.52 / 28.23 |
| tiny_object / retain-stringify | 200,000 | 36.39 / 36.34 / 71.11 / 46.97 |
| small_record / retain-parse | 1 | 12.34 / 12.25 / 51.53 / 28.25 |
| small_record / retain-parse | 100,000 | 57.00 / 56.97 / 85.39 / 52.62 |
| small_record / retain-stringify | 1 | 12.64 / 12.52 / 51.64 / 28.28 |
| small_record / retain-stringify | 100,000 | 31.39 / 31.31 / 80.28 / 51.03 |
| records_array_1m / retain-parse | 1 | 17.95 / 17.83 / 57.28 / 31.59 |
| records_array_1m / retain-parse | 16 | 41.38 / 41.25 / 87.53 / 55.88 |
| records_array_1m / retain-stringify | 1 | 20.17 / 20.03 / 59.36 / 34.41 |
| records_array_1m / retain-stringify | 16 | 34.06 / 33.94 / 75.20 / 47.17 |
| records_object_1m / retain-parse | 1 | 15.98 / 15.88 / 57.14 / 31.61 |
| records_object_1m / retain-parse | 16 | 67.20 / 67.05 / 87.61 / 55.88 |
| records_object_1m / retain-stringify | 1 | 20.20 / 20.08 / 59.36 / 34.38 |
| records_object_1m / retain-stringify | 16 | 34.09 / 33.97 / 75.17 / 47.19 |
| records_array_8m / retain-parse | 1 | 48.64 / 48.52 / 93.14 / 52.28 |
| records_array_8m / retain-parse | 4 | 89.30 / 89.22 / 145.80 / 91.75 |
| records_array_8m / retain-stringify | 1 | 75.75 / 75.66 / 113.00 / 73.41 |
| records_array_8m / retain-stringify | 4 | 95.05 / 94.95 / 142.91 / 94.41 |
| records_object_8m / retain-parse | 1 | 42.91 / 42.94 / 93.17 / 52.28 |
| records_object_8m / retain-parse | 4 | 113.47 / 113.44 / 145.88 / 91.70 |
| records_object_8m / retain-stringify | 1 | 75.86 / 75.78 / 112.98 / 73.42 |
| records_object_8m / retain-stringify | 4 | 95.16 / 95.08 / 142.77 / 94.44 |
| long_string_1m / retain-parse | 1 | 15.41 / 15.31 / 55.38 / 30.50 |
| long_string_1m / retain-parse | 32 | 56.94 / 56.88 / 87.97 / 61.62 |
| long_string_1m / retain-stringify | 1 | 21.12 / 21.05 / 59.33 / 32.58 |
| long_string_1m / retain-stringify | 32 | 59.86 / 59.75 / 91.06 / 63.67 |
| unicode_1m / retain-parse | 1 | 14.69 / 14.45 / 56.48 / 31.19 |
| unicode_1m / retain-parse | 32 | 44.77 / 44.67 / 85.11 / 58.91 |
| unicode_1m / retain-stringify | 1 | 18.69 / 18.58 / 59.61 / 34.00 |
| unicode_1m / retain-stringify | 32 | 48.62 / 48.52 / 87.98 / 61.72 |
| wide_1m / retain-parse | 1 | 24.58 / 24.09 / 64.41 / 36.25 |
| wide_1m / retain-parse | 16 | 75.66 / 71.27 / 111.28 / 68.83 |
| wide_1m / retain-stringify | 1 | 30.70 / 30.20 / 67.31 / 39.19 |
| wide_1m / retain-stringify | 16 | 45.97 / 45.47 / 85.31 / 53.42 |

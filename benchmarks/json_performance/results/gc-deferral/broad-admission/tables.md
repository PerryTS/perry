Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.47 / 13.64 / 57.55 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.61 / 13.78 / 59.61 / 129.78 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.47 / 13.64 / 57.58 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.011 / 0.030 / 0.029 | 0.91× | 13.62 / 13.78 / 59.50 / 129.75 |
| empty_object / parse | 2,000,000 | 0.057 / 0.071 / 0.045 / 0.024 | 0.81× | 34.77 / 34.61 / 59.44 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.047 / 0.047 / 0.032 / 0.030 | 0.99× | 13.78 / 13.91 / 59.50 / 129.77 |
| tiny_object / parse | 2,000,000 | 0.122 / 0.132 / 0.082 / 0.045 | 0.92× | 34.80 / 34.62 / 59.53 / 69.06 |
| tiny_object / stringify | 2,000,000 | 0.093 / 0.092 / 0.037 / 0.040 | 1.00× | 34.83 / 34.84 / 59.55 / 129.78 |
| small_record / parse | 526,624 | 0.544 / 0.562 / 0.347 / 0.253 | 0.97× | 34.53 / 34.56 / 59.59 / 79.84 |
| small_record / stringify | 1,283,197 | 0.284 / 0.283 / 0.109 / 0.121 | 1.00× | 35.98 / 36.00 / 59.75 / 256.20 |
| object_1k / parse | 518,544 | 0.542 / 0.555 / 0.543 / 0.238 | 0.98× | 35.00 / 35.03 / 61.61 / 71.09 |
| object_1k / stringify | 648,882 | 0.237 / 0.236 / 0.200 / 0.216 | 1.00× | 35.94 / 35.94 / 61.70 / 70.78 |
| records_array_16k / parse | 6,617 | 23.844 / 24.039 / 39.648 / 33.945 | 0.99× | 64.83 / 65.02 / 65.73 / 70.44 |
| records_array_16k / stringify | 9,862 | 23.534 / 23.485 / 13.292 / 23.273 | 1.00× | 36.27 / 36.23 / 61.75 / 70.98 |
| records_array_1m / parse | 96 | 1,552.000 / 1,565.938 / 2,841.938 / 2,135.375 | 0.99× | 62.06 / 62.22 / 93.55 / 79.11 |
| records_array_1m / stringify | 174 | 1,497.540 / 1,511.897 / 843.098 / 971.247 | 0.99× | 59.41 / 59.50 / 106.52 / 100.28 |
| records_object_1m / parse | 70 | 2,842.086 / 2,882.500 / 2,710.143 / 2,139.457 | 0.99× | 58.70 / 58.69 / 92.62 / 75.88 |
| records_object_1m / stringify | 175 | 1,501.560 / 1,507.434 / 844.286 / 970.474 | 1.00× | 59.44 / 59.50 / 107.41 / 100.28 |
| records_array_8m / parse | 10 | 13,390.500 / 13,503.200 / 34,826.700 / 20,838.400 | 0.99× | 99.44 / 100.16 / 244.03 / 131.45 |
| records_array_8m / stringify | 20 | 12,958.350 / 13,066.250 / 6,790.050 / 8,405.650 | 0.99× | 193.72 / 193.84 / 185.95 / 169.84 |
| records_object_8m / parse | 8 | 21,618.125 / 27,592.250 / 30,226.125 / 21,643.000 | 0.78× | 166.88 / 172.89 / 225.44 / 111.08 |
| records_object_8m / stringify | 20 | 12,991.250 / 13,050.850 / 6,810.250 / 8,424.750 | 1.00× | 193.77 / 193.86 / 186.17 / 169.67 |
| records_array_20m / parse | 3 | 53,319.000 / 52,790.667 / 98,339.000 / 56,742.333 | 1.01× | 224.30 / 223.78 / 325.89 / 187.66 |
| records_array_20m / stringify | 8 | 81,758.500 / 82,006.625 / 17,345.625 / 21,065.250 | 1.00× | 158.78 / 158.83 / 392.17 / 304.67 |
| records_object_20m / parse | 3 | 53,374.333 / 52,748.000 / 96,884.667 / 56,621.000 | 1.01× | 224.30 / 223.75 / 326.06 / 188.06 |
| records_object_20m / stringify | 8 | 81,983.250 / 82,125.500 / 17,385.000 / 20,915.625 | 1.00× | 158.83 / 158.84 / 392.09 / 304.84 |
| numbers_1m / parse | 92 | 1,739.565 / 1,712.696 / 3,117.772 / 3,194.120 | 1.02× | 64.88 / 65.08 / 87.28 / 68.19 |
| numbers_1m / stringify | 96 | 1,605.896 / 1,608.750 / 1,977.615 / 2,955.542 | 1.00× | 63.48 / 63.59 / 92.12 / 72.25 |
| long_string_1m / parse | 1,298 | 202.191 / 204.016 / 368.797 / 66.089 | 0.99× | 60.75 / 60.92 / 159.91 / 124.38 |
| long_string_1m / stringify | 1,169 | 171.951 / 174.434 / 105.653 / 97.634 | 0.99× | 61.94 / 62.11 / 158.92 / 163.58 |
| escaped_1m / parse | 82 | 2,038.963 / 2,044.744 / 1,733.878 / 2,085.098 | 1.00× | 58.12 / 58.30 / 73.89 / 64.17 |
| escaped_1m / stringify | 170 | 952.206 / 950.365 / 1,890.771 / 2,086.782 | 1.00× | 57.16 / 57.31 / 124.64 / 74.77 |
| unicode_1m / parse | 1,438 | 256.640 / 257.622 / 438.171 / 60.244 | 1.00× | 55.45 / 55.59 / 164.00 / 151.69 |
| unicode_1m / stringify | 1,405 | 137.868 / 139.777 / 413.214 / 449.887 | 0.99× | 57.17 / 57.33 / 157.62 / 125.88 |
| wide_1m / parse | 36 | 20,422.861 / 15,223.500 / 4,987.500 / 4,154.361 | 1.34× | 176.67 / 168.56 / 110.53 / 86.12 |
| wide_1m / stringify | 216 | 1,690.611 / 1,689.713 / 6,343.995 / 665.847 | 1.00× | 70.98 / 71.08 / 116.42 / 84.33 |
| heterogeneous_1m / parse | 80 | 1,932.838 / 1,935.162 / 3,939.088 / 2,969.613 | 1.00× | 69.34 / 69.52 / 92.09 / 80.84 |
| heterogeneous_1m / stringify | 211 | 847.725 / 847.588 / 899.668 / 1,058.678 | 1.00× | 59.22 / 59.34 / 128.84 / 102.27 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.14 / 12.23 / 52.62 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.69 / 24.80 / 74.84 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.30 / 12.39 / 52.67 / 28.31 |
| tiny_object / retain-stringify | 200,000 | 24.84 / 24.92 / 71.20 / 47.03 |
| small_record / retain-parse | 1 | 12.19 / 12.25 / 52.61 / 28.28 |
| small_record / retain-parse | 100,000 | 44.91 / 44.89 / 85.41 / 52.70 |
| small_record / retain-stringify | 1 | 12.36 / 12.44 / 52.75 / 28.31 |
| small_record / retain-stringify | 100,000 | 28.06 / 28.14 / 80.33 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.34 / 16.36 / 58.34 / 31.62 |
| records_array_1m / retain-parse | 16 | 41.53 / 41.56 / 88.73 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.28 / 19.27 / 60.53 / 34.44 |
| records_array_1m / retain-stringify | 16 | 33.16 / 33.16 / 76.33 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.00 / 15.09 / 58.42 / 31.62 |
| records_object_1m / retain-parse | 16 | 56.55 / 57.30 / 88.78 / 55.91 |
| records_object_1m / retain-stringify | 1 | 19.28 / 19.30 / 60.52 / 34.42 |
| records_object_1m / retain-stringify | 16 | 33.19 / 33.19 / 76.36 / 47.25 |
| records_array_8m / retain-parse | 1 | 48.00 / 48.00 / 94.27 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.33 / 86.41 / 146.86 / 91.92 |
| records_array_8m / retain-stringify | 1 | 72.84 / 72.86 / 114.12 / 73.45 |
| records_array_8m / retain-stringify | 4 | 91.58 / 91.59 / 143.89 / 94.48 |
| records_object_8m / retain-parse | 1 | 36.06 / 36.25 / 94.33 / 52.31 |
| records_object_8m / retain-parse | 4 | 97.45 / 102.41 / 146.97 / 91.69 |
| records_object_8m / retain-stringify | 1 | 72.89 / 72.88 / 114.19 / 73.47 |
| records_object_8m / retain-stringify | 4 | 91.62 / 91.61 / 144.02 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.17 / 15.33 / 56.50 / 30.56 |
| long_string_1m / retain-parse | 32 | 56.73 / 56.84 / 89.17 / 61.66 |
| long_string_1m / retain-stringify | 1 | 18.73 / 18.81 / 60.52 / 32.62 |
| long_string_1m / retain-stringify | 32 | 58.69 / 58.81 / 92.25 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.41 / 14.50 / 57.62 / 31.27 |
| unicode_1m / retain-parse | 32 | 44.61 / 44.70 / 86.31 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.69 / 16.75 / 60.88 / 34.05 |
| unicode_1m / retain-stringify | 32 | 46.62 / 46.70 / 89.11 / 61.78 |
| wide_1m / retain-parse | 1 | 24.00 / 24.09 / 65.59 / 36.31 |
| wide_1m / retain-parse | 16 | 71.17 / 71.27 / 112.50 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.19 / 30.20 / 68.58 / 39.25 |
| wide_1m / retain-stringify | 16 | 45.45 / 45.47 / 86.39 / 53.47 |

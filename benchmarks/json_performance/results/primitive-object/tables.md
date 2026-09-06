Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.013 / 0.014 / 0.027 / 0.019 | 0.96× | 13.39 / 13.47 / 57.56 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.48 / 13.56 / 59.53 / 129.73 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 0.96× | 13.39 / 13.47 / 57.58 / 36.11 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.50 / 13.58 / 59.48 / 129.73 |
| empty_object / parse | 2,000,000 | 0.328 / 0.328 / 0.045 / 0.024 | 1.00× | 34.47 / 34.52 / 59.44 / 68.98 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.66 / 13.67 / 59.44 / 129.72 |
| tiny_object / parse | 2,000,000 | 0.366 / 0.374 / 0.082 / 0.045 | 0.98× | 34.48 / 34.52 / 59.45 / 69.00 |
| tiny_object / stringify | 2,000,000 | 0.140 / 0.140 / 0.037 / 0.039 | 1.00× | 34.70 / 34.70 / 59.45 / 129.73 |
| small_record / parse | 587,850 | 0.745 / 0.753 / 0.345 / 0.251 | 0.99× | 34.86 / 34.91 / 59.42 / 79.81 |
| small_record / stringify | 1,259,842 | 0.519 / 0.527 / 0.109 / 0.120 | 0.99× | 35.81 / 35.78 / 59.58 / 252.25 |
| object_1k / parse | 552,063 | 0.591 / 0.592 / 0.540 / 0.238 | 1.00× | 34.86 / 34.94 / 61.53 / 71.12 |
| object_1k / stringify | 646,900 | 0.288 / 0.287 / 0.199 / 0.215 | 1.00× | 35.83 / 35.83 / 61.70 / 70.78 |
| records_array_16k / parse | 6,773 | 23.792 / 23.642 / 42.049 / 33.916 | 1.01× | 64.97 / 64.94 / 65.59 / 70.41 |
| records_array_16k / stringify | 10,055 | 24.332 / 23.963 / 13.170 / 23.167 | 1.02× | 36.12 / 36.08 / 61.81 / 71.00 |
| records_array_1m / parse | 94 | 1,514.798 / 1,506.957 / 2,868.117 / 2,142.234 | 1.01× | 62.23 / 62.23 / 92.84 / 79.08 |
| records_array_1m / stringify | 177 | 1,544.740 / 1,545.006 / 842.237 / 969.571 | 1.00× | 60.47 / 60.48 / 109.25 / 100.30 |
| records_object_1m / parse | 71 | 4,020.014 / 3,969.282 / 2,746.155 / 2,410.958 | 1.01× | 80.00 / 80.02 / 92.89 / 75.91 |
| records_object_1m / stringify | 176 | 1,549.881 / 1,551.614 / 846.455 / 971.068 | 1.00× | 60.47 / 60.45 / 108.38 / 100.27 |
| records_array_8m / parse | 10 | 13,618.300 / 13,524.800 / 33,928.300 / 20,965.000 | 1.01× | 102.69 / 102.67 / 244.17 / 130.95 |
| records_array_8m / stringify | 20 | 13,449.350 / 13,461.350 / 6,861.850 / 8,520.750 | 1.00× | 195.94 / 195.98 / 185.92 / 189.72 |
| records_object_8m / parse | 8 | 31,434.875 / 30,996.250 / 29,668.000 / 21,814.250 | 1.01× | 231.73 / 231.77 / 225.56 / 111.94 |
| records_object_8m / stringify | 20 | 13,443.650 / 13,461.900 / 6,843.250 / 8,505.650 | 1.00× | 195.95 / 195.97 / 185.88 / 169.94 |
| records_array_20m / parse | 3 | 78,365.667 / 76,939.667 / 95,796.000 / 56,483.000 | 1.02× | 315.75 / 315.75 / 325.77 / 188.00 |
| records_array_20m / stringify | 8 | 86,001.250 / 85,871.625 / 17,395.000 / 20,869.875 | 1.00× | 171.28 / 171.25 / 392.17 / 304.91 |
| records_object_20m / parse | 3 | 78,159.667 / 76,929.667 / 95,273.333 / 56,515.667 | 1.02× | 315.75 / 315.75 / 325.88 / 187.66 |
| records_object_20m / stringify | 8 | 85,884.500 / 85,815.000 / 17,346.625 / 20,876.125 | 1.00× | 171.27 / 171.23 / 392.11 / 304.83 |
| numbers_1m / parse | 87 | 1,728.011 / 1,730.000 / 3,123.943 / 3,205.253 | 1.00× | 64.52 / 64.50 / 82.62 / 65.14 |
| numbers_1m / stringify | 96 | 1,606.562 / 1,605.260 / 1,981.604 / 2,955.885 | 1.00× | 61.36 / 61.38 / 92.31 / 72.20 |
| long_string_1m / parse | 1,329 | 205.856 / 203.132 / 368.544 / 67.798 | 1.01× | 60.83 / 60.83 / 160.98 / 160.38 |
| long_string_1m / stringify | 1,166 | 174.864 / 173.629 / 105.715 / 97.389 | 1.01× | 62.02 / 62.00 / 157.98 / 155.61 |
| escaped_1m / parse | 82 | 2,054.659 / 2,043.976 / 1,733.146 / 2,083.341 | 1.01× | 58.22 / 58.22 / 74.06 / 64.17 |
| escaped_1m / stringify | 83 | 1,860.663 / 1,893.205 / 1,858.578 / 2,094.458 | 0.98× | 57.08 / 56.98 / 79.59 / 68.98 |
| unicode_1m / parse | 1,537 | 258.011 / 257.223 / 437.615 / 60.269 | 1.00× | 55.53 / 55.55 / 166.75 / 156.20 |
| unicode_1m / stringify | 1,378 | 139.089 / 139.578 / 412.603 / 450.591 | 1.00× | 57.25 / 57.23 / 156.94 / 158.88 |
| wide_1m / parse | 36 | 20,892.778 / 21,011.500 / 4,950.194 / 4,154.639 | 0.99× | 176.66 / 176.72 / 110.50 / 86.12 |
| wide_1m / stringify | 215 | 2,487.670 / 1,825.247 / 6,367.791 / 664.707 | 1.36× | 70.95 / 70.94 / 116.42 / 83.19 |
| heterogeneous_1m / parse | 77 | 1,914.026 / 1,894.039 / 3,912.494 / 2,992.753 | 1.01× | 68.12 / 68.12 / 92.72 / 80.56 |
| heterogeneous_1m / stringify | 168 | 4,533.101 / 4,520.589 / 909.762 / 1,060.387 | 1.00× | 59.23 / 59.19 / 110.62 / 101.41 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.20 / 12.19 / 52.59 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.83 / 24.73 / 74.75 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.31 / 12.33 / 52.78 / 28.36 |
| tiny_object / retain-stringify | 200,000 | 24.92 / 24.86 / 71.19 / 47.02 |
| small_record / retain-parse | 1 | 12.23 / 12.23 / 52.61 / 28.28 |
| small_record / retain-parse | 100,000 | 56.91 / 56.80 / 85.48 / 52.69 |
| small_record / retain-stringify | 1 | 12.50 / 12.47 / 52.77 / 28.34 |
| small_record / retain-stringify | 100,000 | 31.34 / 31.27 / 80.28 / 51.03 |
| records_array_1m / retain-parse | 1 | 17.86 / 17.83 / 58.48 / 31.61 |
| records_array_1m / retain-parse | 16 | 41.28 / 41.23 / 88.70 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.09 / 20.02 / 60.62 / 34.41 |
| records_array_1m / retain-stringify | 16 | 33.97 / 33.91 / 76.30 / 47.20 |
| records_object_1m / retain-parse | 1 | 15.92 / 15.86 / 58.41 / 31.62 |
| records_object_1m / retain-parse | 16 | 67.02 / 66.92 / 88.66 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.08 / 20.02 / 60.52 / 34.47 |
| records_object_1m / retain-stringify | 16 | 33.97 / 33.91 / 76.31 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.52 / 48.45 / 94.30 / 52.31 |
| records_array_8m / retain-parse | 4 | 89.31 / 89.17 / 146.84 / 91.83 |
| records_array_8m / retain-stringify | 1 | 75.66 / 75.50 / 114.14 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.95 / 94.80 / 143.95 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.89 / 42.88 / 94.33 / 52.31 |
| records_object_8m / retain-parse | 4 | 113.38 / 113.27 / 146.81 / 91.80 |
| records_object_8m / retain-stringify | 1 | 75.66 / 75.50 / 114.20 / 73.44 |
| records_object_8m / retain-stringify | 4 | 94.95 / 94.80 / 143.86 / 94.45 |
| long_string_1m / retain-parse | 1 | 15.23 / 15.30 / 56.53 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.92 / 56.75 / 89.14 / 61.62 |
| long_string_1m / retain-stringify | 1 | 18.73 / 18.73 / 60.59 / 32.64 |
| long_string_1m / retain-stringify | 32 | 58.91 / 58.77 / 92.28 / 63.70 |
| unicode_1m / retain-parse | 1 | 14.52 / 14.47 / 57.61 / 31.23 |
| unicode_1m / retain-parse | 32 | 44.72 / 44.67 / 86.36 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.73 / 16.70 / 60.88 / 34.47 |
| unicode_1m / retain-stringify | 32 | 46.70 / 46.66 / 89.16 / 61.77 |
| wide_1m / retain-parse | 1 | 24.09 / 24.08 / 65.58 / 36.27 |
| wide_1m / retain-parse | 16 | 71.27 / 71.25 / 112.45 / 68.86 |
| wide_1m / retain-stringify | 1 | 30.17 / 30.14 / 68.59 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.55 / 45.41 / 86.47 / 53.45 |

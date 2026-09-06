Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.013 / 0.015 / 0.027 / 0.019 | 0.90× | 13.39 / 13.44 / 57.53 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.48 / 13.56 / 59.56 / 129.73 |
| string_a / parse | 2,000,000 | 0.016 / 0.017 / 0.032 / 0.023 | 0.91× | 13.39 / 13.44 / 57.55 / 36.12 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.48 / 13.56 / 59.56 / 129.75 |
| empty_object / parse | 2,000,000 | 0.328 / 0.328 / 0.045 / 0.024 | 1.00× | 34.48 / 34.66 / 59.39 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.62 / 13.66 / 59.53 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.366 / 0.366 / 0.082 / 0.045 | 1.00× | 34.48 / 34.67 / 59.47 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.140 / 0.140 / 0.037 / 0.039 | 1.00× | 34.70 / 34.88 / 59.58 / 129.73 |
| small_record / parse | 582,382 | 0.745 / 0.728 / 0.368 / 0.252 | 1.02× | 34.88 / 35.06 / 59.66 / 79.83 |
| small_record / stringify | 1,267,828 | 0.519 / 0.525 / 0.108 / 0.121 | 0.99× | 35.84 / 35.98 / 59.61 / 253.61 |
| object_1k / parse | 543,436 | 0.592 / 0.585 / 0.541 / 0.237 | 1.01× | 34.91 / 35.09 / 61.55 / 71.09 |
| object_1k / stringify | 637,393 | 0.289 / 0.289 / 0.199 / 0.216 | 1.00× | 35.83 / 36.00 / 61.70 / 70.84 |
| records_array_16k / parse | 6,557 | 23.585 / 25.827 / 39.419 / 33.980 | 0.91× | 64.97 / 65.05 / 65.67 / 70.41 |
| records_array_16k / stringify | 9,836 | 24.083 / 24.021 / 13.203 / 23.220 | 1.00× | 36.22 / 36.34 / 61.84 / 71.02 |
| records_array_1m / parse | 94 | 1,514.702 / 1,642.202 / 2,741.340 / 2,134.628 | 0.92× | 62.22 / 62.30 / 92.67 / 79.12 |
| records_array_1m / stringify | 177 | 1,545.226 / 1,551.458 / 847.175 / 970.379 | 1.00× | 60.69 / 60.73 / 109.09 / 100.27 |
| records_object_1m / parse | 71 | 4,016.493 / 3,964.352 / 2,696.620 / 2,131.366 | 1.01× | 80.28 / 80.47 / 92.70 / 75.95 |
| records_object_1m / stringify | 176 | 1,555.227 / 1,551.841 / 849.585 / 968.426 | 1.00× | 60.69 / 60.73 / 108.36 / 100.25 |
| records_array_8m / parse | 10 | 13,588.400 / 14,389.200 / 34,945.900 / 21,003.800 | 0.94× | 102.69 / 102.80 / 244.09 / 131.20 |
| records_array_8m / stringify | 21 | 13,368.429 / 13,346.619 / 6,771.048 / 8,442.810 | 1.00× | 195.89 / 196.02 / 186.19 / 176.42 |
| records_object_8m / parse | 8 | 31,535.875 / 31,401.750 / 29,795.625 / 21,889.625 | 1.00× | 231.64 / 231.78 / 225.47 / 111.69 |
| records_object_8m / stringify | 20 | 13,434.900 / 13,452.100 / 6,811.500 / 8,495.000 | 1.00× | 195.89 / 196.02 / 186.03 / 189.92 |
| records_array_20m / parse | 3 | 78,322.333 / 77,819.667 / 93,460.667 / 56,506.000 | 1.01× | 315.61 / 315.75 / 326.25 / 188.17 |
| records_array_20m / stringify | 8 | 85,762.875 / 85,579.625 / 17,354.125 / 20,849.250 | 1.00× | 176.06 / 176.16 / 392.09 / 304.75 |
| records_object_20m / parse | 3 | 78,396.333 / 77,411.000 / 96,073.667 / 56,890.000 | 1.01× | 315.75 / 315.84 / 326.08 / 187.97 |
| records_object_20m / stringify | 8 | 85,983.500 / 85,756.875 / 17,401.125 / 20,919.375 | 1.00× | 171.27 / 171.30 / 392.16 / 304.75 |
| numbers_1m / parse | 89 | 1,721.371 / 1,688.978 / 3,124.640 / 3,185.944 | 1.02× | 64.52 / 64.61 / 84.33 / 65.17 |
| numbers_1m / stringify | 96 | 1,606.729 / 1,607.344 / 1,984.677 / 2,948.917 | 1.00× | 61.34 / 61.41 / 92.09 / 72.19 |
| long_string_1m / parse | 1,249 | 204.855 / 204.428 / 369.258 / 66.114 | 1.00× | 60.86 / 60.97 / 157.81 / 124.44 |
| long_string_1m / stringify | 1,136 | 171.820 / 172.203 / 105.606 / 97.614 | 1.00× | 62.02 / 62.09 / 156.86 / 159.58 |
| escaped_1m / parse | 82 | 2,052.439 / 3,091.634 / 1,732.829 / 2,082.976 | 0.66× | 58.22 / 58.33 / 73.86 / 64.17 |
| escaped_1m / stringify | 84 | 1,862.000 / 1,905.357 / 1,853.976 / 2,099.417 | 0.98× | 57.08 / 57.11 / 80.56 / 69.02 |
| unicode_1m / parse | 1,469 | 257.483 / 256.705 / 438.012 / 60.037 | 1.00× | 55.55 / 55.62 / 164.84 / 154.39 |
| unicode_1m / stringify | 1,397 | 137.715 / 138.208 / 412.766 / 448.176 | 1.00× | 57.25 / 57.33 / 157.55 / 131.17 |
| wide_1m / parse | 36 | 20,893.278 / 20,785.528 / 4,931.806 / 4,163.500 | 1.01× | 176.66 / 176.80 / 110.61 / 86.12 |
| wide_1m / stringify | 215 | 2,485.944 / 2,480.772 / 6,367.665 / 664.981 | 1.00× | 70.95 / 71.03 / 116.62 / 83.33 |
| heterogeneous_1m / parse | 77 | 1,913.519 / 1,909.753 / 3,974.740 / 2,984.857 | 1.00× | 68.02 / 68.31 / 92.48 / 80.56 |
| heterogeneous_1m / stringify | 168 | 4,539.327 / 4,524.970 / 907.327 / 1,059.429 | 1.00× | 59.22 / 59.25 / 110.50 / 101.42 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.20 / 12.20 / 52.61 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.83 / 24.80 / 74.84 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.31 / 12.33 / 52.69 / 28.28 |
| tiny_object / retain-stringify | 200,000 | 24.91 / 24.89 / 71.27 / 47.02 |
| small_record / retain-parse | 1 | 12.27 / 12.23 / 52.69 / 28.27 |
| small_record / retain-parse | 100,000 | 56.91 / 56.98 / 85.73 / 52.66 |
| small_record / retain-stringify | 1 | 12.50 / 12.44 / 52.80 / 28.30 |
| small_record / retain-stringify | 100,000 | 31.38 / 31.27 / 80.36 / 51.05 |
| records_array_1m / retain-parse | 1 | 17.86 / 17.86 / 58.44 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.28 / 41.27 / 88.84 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.08 / 20.00 / 60.61 / 34.42 |
| records_array_1m / retain-stringify | 16 | 33.97 / 33.89 / 76.39 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.92 / 15.88 / 58.39 / 31.66 |
| records_object_1m / retain-parse | 16 | 66.95 / 67.02 / 88.73 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.08 / 19.98 / 60.50 / 34.44 |
| records_object_1m / retain-stringify | 16 | 33.95 / 33.89 / 76.36 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.55 / 48.52 / 94.27 / 52.30 |
| records_array_8m / retain-parse | 4 | 89.34 / 89.28 / 146.94 / 91.92 |
| records_array_8m / retain-stringify | 1 | 75.59 / 75.61 / 114.16 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.89 / 94.92 / 143.98 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.97 / 42.81 / 94.30 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.36 / 113.45 / 146.98 / 91.92 |
| records_object_8m / retain-stringify | 1 | 75.70 / 75.72 / 114.09 / 73.44 |
| records_object_8m / retain-stringify | 4 | 95.00 / 95.03 / 143.72 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.33 / 15.31 / 56.48 / 30.52 |
| long_string_1m / retain-parse | 32 | 56.97 / 56.91 / 89.09 / 61.66 |
| long_string_1m / retain-stringify | 1 | 18.78 / 18.78 / 60.45 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.94 / 58.84 / 92.27 / 63.72 |
| unicode_1m / retain-parse | 1 | 14.52 / 14.48 / 57.61 / 31.23 |
| unicode_1m / retain-parse | 32 | 44.73 / 44.69 / 86.27 / 58.97 |
| unicode_1m / retain-stringify | 1 | 16.72 / 16.73 / 60.83 / 34.50 |
| unicode_1m / retain-stringify | 32 | 46.70 / 46.67 / 89.12 / 61.77 |
| wide_1m / retain-parse | 1 | 24.09 / 24.09 / 65.58 / 36.28 |
| wide_1m / retain-parse | 16 | 71.27 / 71.27 / 112.44 / 68.84 |
| wide_1m / retain-stringify | 1 | 30.17 / 30.16 / 68.56 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.44 / 45.53 / 86.50 / 53.44 |

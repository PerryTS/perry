Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.250 / 0.251 / 0.027 / 0.019 | 1.00× | 13.56 / 13.55 / 57.56 / 35.62 |
| null / stringify | 2,000,000 | 0.063 / 0.066 / 0.027 / 0.029 | 0.96× | 34.91 / 34.89 / 59.53 / 129.73 |
| string_a / parse | 2,000,000 | 0.254 / 0.254 / 0.032 / 0.023 | 1.00× | 13.56 / 13.55 / 57.58 / 36.03 |
| string_a / stringify | 2,000,000 | 0.072 / 0.074 / 0.030 / 0.029 | 0.96× | 34.88 / 34.86 / 59.50 / 129.75 |
| empty_object / parse | 2,000,000 | 0.325 / 0.326 / 0.045 / 0.024 | 1.00× | 34.67 / 34.64 / 59.45 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.135 / 0.136 / 0.032 / 0.030 | 0.99× | 34.92 / 34.92 / 59.59 / 129.72 |
| tiny_object / parse | 2,000,000 | 0.364 / 0.363 / 0.082 / 0.046 | 1.00× | 34.67 / 34.64 / 59.44 / 69.00 |
| tiny_object / stringify | 2,000,000 | 0.194 / 0.196 / 0.037 / 0.040 | 0.99× | 34.94 / 34.92 / 59.58 / 129.72 |
| small_record / parse | 579,243 | 0.755 / 0.750 / 0.357 / 0.252 | 1.01× | 35.05 / 35.02 / 59.61 / 79.84 |
| small_record / stringify | 1,275,461 | 0.556 / 0.550 / 0.109 / 0.121 | 1.01× | 36.00 / 35.98 / 59.72 / 254.88 |
| object_1k / parse | 543,765 | 0.592 / 0.592 / 0.543 / 0.238 | 1.00× | 35.09 / 35.02 / 61.75 / 71.11 |
| object_1k / stringify | 647,249 | 0.385 / 0.385 / 0.199 / 0.215 | 1.00× | 36.09 / 36.06 / 61.72 / 70.81 |
| records_array_16k / parse | 6,691 | 23.774 / 23.787 / 40.515 / 33.968 | 1.00× | 65.00 / 65.02 / 65.62 / 70.38 |
| records_array_16k / stringify | 10,055 | 35.674 / 35.791 / 13.247 / 23.185 | 1.00× | 36.38 / 36.36 / 61.78 / 70.98 |
| records_array_1m / parse | 92 | 1,542.663 / 1,543.076 / 2,935.109 / 2,133.370 | 1.00× | 62.27 / 62.27 / 92.73 / 79.06 |
| records_array_1m / stringify | 174 | 2,204.655 / 2,202.046 / 842.920 / 969.351 | 1.00× | 61.58 / 60.77 / 106.45 / 100.27 |
| records_object_1m / parse | 71 | 4,022.648 / 4,013.394 / 2,947.310 / 2,135.056 | 1.00× | 80.47 / 80.42 / 92.70 / 75.89 |
| records_object_1m / stringify | 177 | 2,206.712 / 2,198.655 / 843.644 / 969.650 | 1.00× | 61.58 / 60.77 / 109.16 / 100.28 |
| records_array_8m / parse | 10 | 13,805.800 / 13,804.300 / 34,222.600 / 20,791.000 | 1.00× | 102.75 / 102.73 / 243.97 / 131.34 |
| records_array_8m / stringify | 20 | 18,661.250 / 18,711.850 / 6,826.550 / 8,437.750 | 1.00× | 196.00 / 195.98 / 185.83 / 169.61 |
| records_object_8m / parse | 8 | 31,577.375 / 31,461.750 / 30,233.875 / 21,748.375 | 1.00× | 231.75 / 231.72 / 225.50 / 111.73 |
| records_object_8m / stringify | 20 | 18,694.000 / 18,720.600 / 6,801.300 / 8,454.450 | 1.00× | 196.00 / 195.98 / 186.05 / 190.05 |
| records_array_20m / parse | 3 | 78,782.333 / 77,897.333 / 97,399.333 / 56,536.000 | 1.01× | 315.73 / 315.67 / 326.27 / 187.89 |
| records_array_20m / stringify | 8 | 97,126.500 / 97,360.500 / 17,358.500 / 20,798.625 | 1.00× | 176.16 / 176.16 / 392.06 / 304.89 |
| records_object_20m / parse | 3 | 78,510.333 / 78,132.000 / 95,937.333 / 56,941.000 | 1.00× | 315.73 / 315.67 / 325.69 / 187.78 |
| records_object_20m / stringify | 8 | 97,334.750 / 97,144.125 / 17,346.250 / 20,920.875 | 1.00× | 176.16 / 176.14 / 392.09 / 304.86 |
| numbers_1m / parse | 95 | 1,564.084 / 1,560.663 / 3,119.453 / 3,204.453 | 1.00× | 64.53 / 64.84 / 90.27 / 68.16 |
| numbers_1m / stringify | 75 | 6,597.653 / 6,641.760 / 1,935.147 / 2,951.227 | 0.99× | 61.44 / 61.44 / 81.73 / 70.17 |
| long_string_1m / parse | 1,357 | 204.577 / 204.198 / 368.674 / 67.437 | 1.00× | 62.94 / 60.92 / 161.98 / 156.39 |
| long_string_1m / stringify | 1,041 | 205.481 / 203.076 / 106.367 / 98.020 | 1.01× | 64.98 / 64.02 / 153.80 / 155.55 |
| escaped_1m / parse | 82 | 2,039.390 / 2,039.329 / 1,733.256 / 2,086.195 | 1.00× | 58.28 / 58.30 / 74.05 / 64.14 |
| escaped_1m / stringify | 83 | 1,862.386 / 1,860.277 / 1,850.614 / 2,096.325 | 1.00× | 57.03 / 57.06 / 79.48 / 68.97 |
| unicode_1m / parse | 1,469 | 984.182 / 257.028 / 438.053 / 59.061 | 3.83× | 56.47 / 55.61 / 164.88 / 123.14 |
| unicode_1m / stringify | 664 | 982.020 / 256.346 / 420.315 / 451.248 | 3.83× | 60.02 / 58.33 / 138.53 / 97.23 |
| wide_1m / parse | 36 | 20,998.528 / 20,964.556 / 4,939.861 / 4,159.278 | 1.00× | 176.81 / 176.77 / 110.58 / 86.09 |
| wide_1m / stringify | 214 | 2,496.715 / 2,473.486 / 6,317.888 / 665.902 | 1.01× | 70.97 / 71.00 / 116.44 / 84.61 |
| heterogeneous_1m / parse | 79 | 1,865.671 / 1,866.430 / 3,915.038 / 2,965.354 | 1.00× | 68.05 / 68.06 / 92.14 / 80.86 |
| heterogeneous_1m / stringify | 168 | 4,499.256 / 4,498.702 / 899.911 / 1,058.446 | 1.00× | 60.03 / 59.27 / 110.42 / 101.50 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.22 / 12.33 / 52.59 / 28.23 |
| tiny_object / retain-parse | 200,000 | 24.78 / 24.89 / 74.64 / 50.14 |
| tiny_object / retain-stringify | 1 | 12.47 / 12.56 / 52.78 / 28.28 |
| tiny_object / retain-stringify | 200,000 | 36.34 / 36.41 / 71.25 / 47.00 |
| small_record / retain-parse | 1 | 12.25 / 12.34 / 52.67 / 28.25 |
| small_record / retain-parse | 100,000 | 56.98 / 57.02 / 85.47 / 52.69 |
| small_record / retain-stringify | 1 | 12.52 / 12.61 / 52.78 / 28.34 |
| small_record / retain-stringify | 100,000 | 31.33 / 31.41 / 80.33 / 51.02 |
| records_array_1m / retain-parse | 1 | 17.84 / 17.97 / 58.38 / 31.59 |
| records_array_1m / retain-parse | 16 | 41.25 / 41.38 / 88.69 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.05 / 20.16 / 60.55 / 34.41 |
| records_array_1m / retain-stringify | 16 | 33.94 / 34.05 / 76.36 / 47.19 |
| records_object_1m / retain-parse | 1 | 15.89 / 15.98 / 58.33 / 31.62 |
| records_object_1m / retain-parse | 16 | 67.06 / 67.11 / 88.66 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.09 / 20.17 / 60.56 / 34.39 |
| records_object_1m / retain-stringify | 16 | 33.97 / 34.06 / 76.36 / 47.20 |
| records_array_8m / retain-parse | 1 | 48.52 / 48.62 / 94.25 / 52.27 |
| records_array_8m / retain-parse | 4 | 89.22 / 89.33 / 146.83 / 91.89 |
| records_array_8m / retain-stringify | 1 | 75.66 / 75.70 / 114.22 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.95 / 95.00 / 143.95 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.94 / 42.92 / 94.34 / 52.28 |
| records_object_8m / retain-parse | 4 | 113.38 / 113.47 / 146.69 / 91.77 |
| records_object_8m / retain-stringify | 1 | 75.78 / 75.81 / 114.14 / 73.42 |
| records_object_8m / retain-stringify | 4 | 95.08 / 95.11 / 143.97 / 94.45 |
| long_string_1m / retain-parse | 1 | 15.31 / 15.44 / 56.50 / 30.58 |
| long_string_1m / retain-parse | 32 | 56.88 / 56.98 / 89.08 / 61.62 |
| long_string_1m / retain-stringify | 1 | 21.05 / 21.14 / 60.55 / 32.58 |
| long_string_1m / retain-stringify | 32 | 59.75 / 59.89 / 92.31 / 63.72 |
| unicode_1m / retain-parse | 1 | 14.45 / 14.62 / 57.69 / 31.22 |
| unicode_1m / retain-parse | 32 | 44.67 / 44.84 / 86.22 / 58.92 |
| unicode_1m / retain-stringify | 1 | 18.58 / 18.73 / 60.83 / 34.05 |
| unicode_1m / retain-stringify | 32 | 48.53 / 48.69 / 89.11 / 61.73 |
| wide_1m / retain-parse | 1 | 24.09 / 24.20 / 65.56 / 36.31 |
| wide_1m / retain-parse | 16 | 71.27 / 71.38 / 112.42 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.20 / 30.28 / 68.61 / 39.20 |
| wide_1m / retain-stringify | 16 | 45.47 / 45.55 / 86.39 / 53.44 |

Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.251 / 0.250 / 0.027 / 0.019 | 1.00× | 13.55 / 13.44 / 57.58 / 35.66 |
| null / stringify | 2,000,000 | 0.066 / 0.007 / 0.027 / 0.029 | 10.04× | 34.89 / 13.50 / 59.48 / 129.72 |
| string_a / parse | 2,000,000 | 0.254 / 0.253 / 0.032 / 0.023 | 1.01× | 13.55 / 13.44 / 57.53 / 36.02 |
| string_a / stringify | 2,000,000 | 0.074 / 0.010 / 0.030 / 0.029 | 7.68× | 34.86 / 13.50 / 59.44 / 129.70 |
| empty_object / parse | 2,000,000 | 0.325 / 0.324 / 0.045 / 0.024 | 1.00× | 34.64 / 34.41 / 59.44 / 68.97 |
| empty_object / stringify | 2,000,000 | 0.136 / 0.125 / 0.032 / 0.030 | 1.09× | 34.92 / 34.70 / 59.56 / 129.70 |
| tiny_object / parse | 2,000,000 | 0.363 / 0.362 / 0.082 / 0.045 | 1.00× | 34.64 / 34.41 / 59.45 / 69.03 |
| tiny_object / stringify | 2,000,000 | 0.196 / 0.197 / 0.037 / 0.040 | 0.99× | 34.92 / 34.72 / 59.50 / 129.70 |
| small_record / parse | 586,988 | 0.750 / 0.750 / 0.339 / 0.253 | 1.00× | 35.02 / 34.77 / 59.55 / 79.83 |
| small_record / stringify | 1,268,053 | 0.551 / 0.538 / 0.108 / 0.120 | 1.03× | 35.94 / 35.75 / 59.67 / 253.58 |
| object_1k / parse | 545,206 | 0.591 / 0.592 / 0.543 / 0.237 | 1.00× | 35.02 / 34.83 / 61.66 / 71.09 |
| object_1k / stringify | 646,319 | 0.385 / 0.387 / 0.200 / 0.215 | 1.00× | 36.08 / 35.83 / 61.72 / 70.78 |
| records_array_16k / parse | 6,716 | 23.774 / 23.805 / 39.921 / 33.983 | 1.00× | 65.02 / 64.78 / 65.67 / 70.38 |
| records_array_16k / stringify | 9,986 | 35.908 / 31.249 / 13.173 / 23.206 | 1.15× | 36.27 / 36.06 / 61.77 / 70.97 |
| records_array_1m / parse | 92 | 1,540.685 / 1,543.511 / 2,877.402 / 2,137.370 | 1.00× | 62.30 / 62.06 / 93.06 / 79.12 |
| records_array_1m / stringify | 176 | 2,199.602 / 1,964.011 / 850.261 / 967.256 | 1.12× | 60.55 / 60.33 / 108.36 / 100.25 |
| records_object_1m / parse | 71 | 4,014.845 / 4,033.845 / 2,754.197 / 2,137.127 | 1.00× | 80.14 / 79.89 / 92.67 / 75.92 |
| records_object_1m / stringify | 176 | 2,206.676 / 1,974.000 / 849.494 / 967.523 | 1.12× | 60.55 / 60.31 / 108.52 / 100.27 |
| records_array_8m / parse | 10 | 13,764.000 / 13,822.300 / 34,114.400 / 20,897.900 | 1.00× | 102.73 / 102.52 / 244.06 / 131.56 |
| records_array_8m / stringify | 20 | 18,724.250 / 16,766.500 / 6,779.950 / 8,385.050 | 1.12× | 196.03 / 195.88 / 186.17 / 169.72 |
| records_object_8m / parse | 8 | 31,671.750 / 31,686.875 / 29,192.500 / 21,903.375 | 1.00× | 231.81 / 231.59 / 225.52 / 111.28 |
| records_object_8m / stringify | 21 | 18,645.571 / 16,691.381 / 6,781.333 / 8,454.667 | 1.12× | 196.05 / 195.86 / 186.03 / 196.52 |
| records_array_20m / parse | 3 | 78,723.667 / 78,895.667 / 96,202.333 / 56,841.000 | 1.00× | 315.81 / 315.58 / 326.19 / 187.86 |
| records_array_20m / stringify | 8 | 97,496.750 / 94,248.125 / 17,337.875 / 20,841.250 | 1.03× | 171.38 / 171.09 / 392.17 / 304.77 |
| records_object_20m / parse | 3 | 78,643.333 / 78,781.000 / 98,066.000 / 56,745.000 | 1.00× | 315.83 / 315.56 / 326.31 / 187.88 |
| records_object_20m / stringify | 8 | 97,644.125 / 94,589.250 / 17,368.500 / 20,812.000 | 1.03× | 171.36 / 171.09 / 392.08 / 303.86 |
| numbers_1m / parse | 96 | 1,577.417 / 1,578.146 / 3,126.854 / 3,202.635 | 1.00× | 64.53 / 67.08 / 91.27 / 68.16 |
| numbers_1m / stringify | 96 | 6,639.448 / 1,607.448 / 1,979.823 / 2,952.708 | 4.13× | 61.45 / 61.20 / 92.36 / 72.22 |
| long_string_1m / parse | 1,357 | 203.432 / 204.525 / 369.234 / 65.709 | 0.99× | 60.92 / 60.64 / 162.11 / 124.39 |
| long_string_1m / stringify | 1,042 | 204.560 / 207.126 / 106.927 / 98.364 | 0.99× | 64.02 / 63.77 / 153.94 / 153.56 |
| escaped_1m / parse | 83 | 2,057.084 / 2,037.458 / 1,732.386 / 2,082.494 | 1.01× | 58.30 / 58.02 / 74.03 / 64.88 |
| escaped_1m / stringify | 83 | 1,862.096 / 1,865.301 / 1,852.795 / 2,100.735 | 1.00× | 57.08 / 56.83 / 79.64 / 68.95 |
| unicode_1m / parse | 1,498 | 256.447 / 256.296 / 437.955 / 60.345 | 1.00× | 55.62 / 55.36 / 165.81 / 156.16 |
| unicode_1m / stringify | 668 | 256.409 / 256.963 / 419.864 / 449.769 | 1.00× | 58.34 / 58.09 / 138.58 / 97.23 |
| wide_1m / parse | 36 | 21,146.139 / 21,602.389 / 4,966.111 / 4,151.528 | 0.98× | 176.75 / 176.53 / 110.61 / 86.11 |
| wide_1m / stringify | 212 | 2,475.024 / 2,483.009 / 6,344.170 / 666.623 | 1.00× | 71.00 / 70.78 / 116.50 / 84.59 |
| heterogeneous_1m / parse | 79 | 1,867.620 / 1,867.494 / 3,831.190 / 2,983.620 | 1.00× | 68.17 / 67.97 / 92.22 / 80.70 |
| heterogeneous_1m / stringify | 161 | 4,507.460 / 4,537.447 / 905.453 / 1,065.894 | 0.99× | 59.27 / 59.06 / 110.62 / 100.50 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.33 / 12.20 / 52.59 / 28.25 |
| tiny_object / retain-parse | 200,000 | 24.91 / 24.78 / 74.73 / 50.16 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.48 / 52.70 / 28.25 |
| tiny_object / retain-stringify | 200,000 | 36.39 / 36.17 / 71.22 / 47.00 |
| small_record / retain-parse | 1 | 12.33 / 12.22 / 52.67 / 28.25 |
| small_record / retain-parse | 100,000 | 57.02 / 56.72 / 85.44 / 52.66 |
| small_record / retain-stringify | 1 | 12.59 / 12.55 / 52.75 / 28.28 |
| small_record / retain-stringify | 100,000 | 31.42 / 31.36 / 80.27 / 51.00 |
| records_array_1m / retain-parse | 1 | 17.95 / 17.91 / 58.45 / 31.62 |
| records_array_1m / retain-parse | 16 | 41.38 / 41.31 / 88.67 / 55.88 |
| records_array_1m / retain-stringify | 1 | 20.17 / 20.11 / 60.58 / 34.39 |
| records_array_1m / retain-stringify | 16 | 34.06 / 34.00 / 76.33 / 47.19 |
| records_object_1m / retain-parse | 1 | 15.98 / 15.88 / 58.44 / 31.61 |
| records_object_1m / retain-parse | 16 | 67.14 / 66.86 / 88.83 / 55.88 |
| records_object_1m / retain-stringify | 1 | 20.17 / 20.11 / 60.53 / 34.41 |
| records_object_1m / retain-stringify | 16 | 34.06 / 34.00 / 76.38 / 47.20 |
| records_array_8m / retain-parse | 1 | 48.62 / 48.53 / 94.25 / 52.28 |
| records_array_8m / retain-parse | 4 | 89.30 / 89.02 / 146.94 / 91.92 |
| records_array_8m / retain-stringify | 1 | 75.77 / 75.53 / 114.12 / 73.42 |
| records_array_8m / retain-stringify | 4 | 95.06 / 94.83 / 144.00 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.94 / 42.73 / 94.30 / 52.28 |
| records_object_8m / retain-parse | 4 | 113.44 / 113.19 / 147.00 / 91.83 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.53 / 114.16 / 73.44 |
| records_object_8m / retain-stringify | 4 | 95.06 / 94.83 / 143.73 / 94.44 |
| long_string_1m / retain-parse | 1 | 15.41 / 15.34 / 56.53 / 30.56 |
| long_string_1m / retain-parse | 32 | 56.94 / 56.66 / 89.14 / 61.59 |
| long_string_1m / retain-stringify | 1 | 21.09 / 21.05 / 60.61 / 32.58 |
| long_string_1m / retain-stringify | 32 | 59.91 / 59.64 / 92.23 / 63.72 |
| unicode_1m / retain-parse | 1 | 14.62 / 14.50 / 57.72 / 31.22 |
| unicode_1m / retain-parse | 32 | 44.84 / 44.72 / 86.27 / 58.92 |
| unicode_1m / retain-stringify | 1 | 18.73 / 18.66 / 60.75 / 34.05 |
| unicode_1m / retain-stringify | 32 | 48.69 / 48.61 / 89.08 / 61.73 |
| wide_1m / retain-parse | 1 | 24.20 / 24.05 / 65.55 / 36.28 |
| wide_1m / retain-parse | 16 | 71.38 / 71.23 / 112.42 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.28 / 30.20 / 68.53 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.55 / 45.47 / 86.53 / 53.44 |

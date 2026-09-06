Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.013 / 0.027 / 0.019 | 1.02× | 13.44 / 13.58 / 57.55 / 35.67 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.52 / 13.66 / 59.52 / 129.78 |
| string_a / parse | 2,000,000 | 0.017 / 0.016 / 0.032 / 0.023 | 1.04× | 13.44 / 13.58 / 57.55 / 36.08 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.52 / 13.67 / 59.47 / 129.78 |
| empty_object / parse | 2,000,000 | 0.057 / 0.057 / 0.045 / 0.024 | 1.00× | 34.64 / 34.81 / 59.52 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.047 / 0.032 / 0.030 | 1.05× | 13.64 / 13.73 / 59.53 / 129.77 |
| tiny_object / parse | 2,000,000 | 0.352 / 0.363 / 0.082 / 0.045 | 0.97× | 34.44 / 34.61 / 59.44 / 69.06 |
| tiny_object / stringify | 2,000,000 | 0.094 / 0.091 / 0.037 / 0.039 | 1.03× | 34.64 / 34.81 / 59.62 / 129.77 |
| small_record / parse | 495,185 | 0.671 / 0.687 / 0.346 / 0.255 | 0.98× | 34.81 / 35.00 / 59.50 / 79.84 |
| small_record / stringify | 1,285,484 | 0.286 / 0.282 / 0.109 / 0.121 | 1.01× | 35.78 / 35.95 / 59.67 / 256.62 |
| object_1k / parse | 557,837 | 0.584 / 0.598 / 0.541 / 0.238 | 0.98× | 34.86 / 35.05 / 61.67 / 71.17 |
| object_1k / stringify | 663,471 | 0.240 / 0.234 / 0.199 / 0.216 | 1.03× | 35.78 / 35.95 / 61.77 / 70.88 |
| records_array_16k / parse | 6,672 | 23.810 / 23.802 / 40.306 / 33.963 | 1.00× | 64.84 / 65.08 / 65.75 / 70.41 |
| records_array_16k / stringify | 10,055 | 24.224 / 23.247 / 13.237 / 23.185 | 1.04× | 36.08 / 36.25 / 61.86 / 71.00 |
| records_array_1m / parse | 94 | 1,553.904 / 1,555.606 / 2,730.330 / 2,134.840 | 1.00× | 62.03 / 62.25 / 92.73 / 79.12 |
| records_array_1m / stringify | 176 | 1,544.778 / 1,500.023 / 844.170 / 970.205 | 1.03× | 60.34 / 60.59 / 108.25 / 100.30 |
| records_object_1m / parse | 69 | 4,008.652 / 4,040.261 / 2,763.812 / 2,138.565 | 0.99× | 79.95 / 80.12 / 92.73 / 75.92 |
| records_object_1m / stringify | 176 | 1,552.489 / 1,506.199 / 842.426 / 969.023 | 1.03× | 60.33 / 60.58 / 108.31 / 100.27 |
| records_array_8m / parse | 10 | 13,376.000 / 13,368.100 / 34,869.700 / 20,797.700 | 1.00× | 99.44 / 99.64 / 243.94 / 131.58 |
| records_array_8m / stringify | 20 | 13,441.050 / 13,096.500 / 6,819.050 / 8,523.700 | 1.03× | 195.92 / 196.14 / 185.95 / 190.00 |
| records_object_8m / parse | 8 | 31,318.750 / 31,354.000 / 29,850.000 / 21,743.250 | 1.00× | 231.70 / 231.89 / 225.47 / 111.31 |
| records_object_8m / stringify | 20 | 13,461.150 / 13,133.400 / 6,827.300 / 8,442.550 | 1.02× | 195.94 / 196.14 / 185.97 / 169.89 |
| records_array_20m / parse | 3 | 77,074.333 / 77,229.333 / 94,743.000 / 56,734.333 | 1.00× | 315.72 / 315.88 / 326.22 / 188.28 |
| records_array_20m / stringify | 8 | 85,829.375 / 85,444.875 / 17,333.375 / 20,760.000 | 1.00× | 171.25 / 171.36 / 392.11 / 304.77 |
| records_object_20m / parse | 3 | 77,395.667 / 77,677.333 / 97,056.667 / 56,790.333 | 1.00× | 315.72 / 315.88 / 325.77 / 187.98 |
| records_object_20m / stringify | 8 | 86,087.625 / 85,245.875 / 17,361.000 / 20,736.250 | 1.01× | 171.23 / 171.38 / 392.16 / 304.67 |
| numbers_1m / parse | 92 | 1,711.815 / 1,711.598 / 3,121.457 / 3,214.783 | 1.00× | 64.86 / 65.09 / 87.27 / 68.22 |
| numbers_1m / stringify | 95 | 1,608.747 / 1,606.442 / 1,978.495 / 2,953.789 | 1.00× | 61.23 / 61.45 / 91.14 / 72.22 |
| long_string_1m / parse | 1,186 | 205.971 / 208.535 / 369.554 / 68.110 | 0.99× | 60.70 / 61.97 / 155.88 / 156.38 |
| long_string_1m / stringify | 1,151 | 174.828 / 174.485 / 105.718 / 96.162 | 1.00× | 61.88 / 62.11 / 157.89 / 123.58 |
| escaped_1m / parse | 82 | 2,043.878 / 2,043.098 / 1,733.561 / 2,083.439 | 1.00× | 58.09 / 58.33 / 73.86 / 64.17 |
| escaped_1m / stringify | 169 | 965.893 / 951.101 / 1,899.491 / 2,088.521 | 1.02× | 57.08 / 57.33 / 124.64 / 74.80 |
| unicode_1m / parse | 1,429 | 257.163 / 258.699 / 438.044 / 59.206 | 0.99× | 55.42 / 56.52 / 163.89 / 123.17 |
| unicode_1m / stringify | 1,424 | 138.009 / 139.402 / 412.555 / 450.279 | 0.99× | 57.11 / 57.34 / 158.67 / 155.30 |
| wide_1m / parse | 36 | 20,886.056 / 20,869.861 / 4,953.972 / 4,173.333 | 1.00× | 176.64 / 176.86 / 110.55 / 86.12 |
| wide_1m / stringify | 213 | 1,709.164 / 1,693.343 / 6,319.056 / 666.840 | 1.01× | 70.83 / 72.03 / 116.36 / 84.67 |
| heterogeneous_1m / parse | 80 | 1,931.275 / 1,935.575 / 3,873.600 / 2,957.688 | 1.00× | 69.33 / 69.55 / 92.12 / 80.83 |
| heterogeneous_1m / stringify | 168 | 4,406.107 / 4,373.119 / 904.542 / 1,059.661 | 1.01× | 59.08 / 60.16 / 110.50 / 100.55 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.20 / 12.31 / 52.62 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.78 / 24.88 / 74.81 / 50.22 |
| tiny_object / retain-stringify | 1 | 12.30 / 12.41 / 52.75 / 28.31 |
| tiny_object / retain-stringify | 200,000 | 24.88 / 24.97 / 71.28 / 47.05 |
| small_record / retain-parse | 1 | 12.20 / 12.31 / 52.64 / 28.30 |
| small_record / retain-parse | 100,000 | 56.83 / 57.00 / 85.47 / 52.69 |
| small_record / retain-stringify | 1 | 12.36 / 12.44 / 52.73 / 28.34 |
| small_record / retain-stringify | 100,000 | 28.09 / 28.16 / 80.33 / 51.05 |
| records_array_1m / retain-parse | 1 | 16.38 / 16.44 / 58.38 / 31.62 |
| records_array_1m / retain-parse | 16 | 41.58 / 41.64 / 88.56 / 55.91 |
| records_array_1m / retain-stringify | 1 | 20.03 / 20.11 / 60.48 / 34.47 |
| records_array_1m / retain-stringify | 16 | 33.91 / 34.02 / 76.28 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.88 / 15.98 / 58.31 / 31.62 |
| records_object_1m / retain-parse | 16 | 66.94 / 67.11 / 88.77 / 55.91 |
| records_object_1m / retain-stringify | 1 | 20.02 / 20.12 / 60.52 / 34.45 |
| records_object_1m / retain-stringify | 16 | 33.91 / 34.02 / 76.31 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.05 / 48.08 / 94.25 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.41 / 86.58 / 146.94 / 91.81 |
| records_array_8m / retain-stringify | 1 | 75.59 / 75.77 / 114.12 / 73.45 |
| records_array_8m / retain-stringify | 4 | 94.89 / 95.06 / 143.97 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.83 / 43.00 / 94.34 / 52.31 |
| records_object_8m / retain-parse | 4 | 113.28 / 113.48 / 146.70 / 92.06 |
| records_object_8m / retain-stringify | 1 | 75.59 / 75.77 / 114.22 / 73.45 |
| records_object_8m / retain-stringify | 4 | 94.89 / 95.06 / 143.95 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.33 / 15.39 / 56.52 / 30.61 |
| long_string_1m / retain-parse | 32 | 56.81 / 57.02 / 89.06 / 61.67 |
| long_string_1m / retain-stringify | 1 | 18.77 / 18.86 / 60.53 / 32.67 |
| long_string_1m / retain-stringify | 32 | 58.75 / 58.92 / 92.27 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.50 / 14.61 / 57.66 / 31.27 |
| unicode_1m / retain-parse | 32 | 44.72 / 44.81 / 86.19 / 58.95 |
| unicode_1m / retain-stringify | 1 | 16.73 / 16.80 / 60.80 / 34.03 |
| unicode_1m / retain-stringify | 32 | 46.69 / 46.77 / 89.16 / 62.20 |
| wide_1m / retain-parse | 1 | 24.11 / 24.20 / 65.55 / 36.30 |
| wide_1m / retain-parse | 16 | 71.28 / 71.38 / 112.48 / 68.91 |
| wide_1m / retain-stringify | 1 | 30.14 / 30.23 / 68.59 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.42 / 45.50 / 86.39 / 53.47 |

Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.250 / 0.250 / 0.027 / 0.019 | 1.00× | 13.55 / 13.47 / 57.36 / 35.61 |
| null / stringify | 2,000,000 | 0.063 / 0.067 / 0.027 / 0.029 | 0.95× | 34.91 / 34.81 / 59.36 / 129.70 |
| string_a / parse | 2,000,000 | 0.254 / 0.254 / 0.032 / 0.023 | 1.00× | 13.58 / 13.48 / 57.28 / 36.00 |
| string_a / stringify | 2,000,000 | 0.072 / 0.075 / 0.030 / 0.029 | 0.96× | 34.88 / 34.78 / 59.39 / 129.72 |
| empty_object / parse | 2,000,000 | 0.325 / 0.326 / 0.045 / 0.024 | 1.00× | 34.67 / 34.59 / 59.17 / 68.97 |
| empty_object / stringify | 2,000,000 | 0.135 / 0.133 / 0.032 / 0.030 | 1.02× | 34.94 / 34.86 / 59.36 / 129.70 |
| tiny_object / parse | 2,000,000 | 0.365 / 0.364 / 0.082 / 0.045 | 1.00× | 34.67 / 34.59 / 59.20 / 68.97 |
| tiny_object / stringify | 2,000,000 | 0.194 / 0.192 / 0.037 / 0.040 | 1.01× | 34.94 / 34.86 / 59.45 / 129.69 |
| small_record / parse | 584,272 | 0.755 / 0.744 / 0.335 / 0.252 | 1.01× | 35.06 / 34.98 / 59.28 / 79.78 |
| small_record / stringify | 1,288,244 | 0.550 / 0.544 / 0.108 / 0.121 | 1.01× | 36.00 / 35.91 / 59.36 / 257.03 |
| object_1k / parse | 552,910 | 0.591 / 0.590 / 0.543 / 0.237 | 1.00× | 35.03 / 35.00 / 61.39 / 71.03 |
| object_1k / stringify | 626,523 | 0.385 / 0.381 / 0.199 / 0.216 | 1.01× | 36.09 / 36.02 / 61.55 / 70.73 |
| records_array_16k / parse | 6,623 | 23.797 / 23.869 / 39.572 / 33.970 | 1.00× | 65.02 / 64.92 / 65.52 / 70.41 |
| records_array_16k / stringify | 10,027 | 35.770 / 35.653 / 13.274 / 23.150 | 1.00× | 36.38 / 36.28 / 61.61 / 70.94 |
| records_array_1m / parse | 93 | 1,544.151 / 1,546.462 / 2,687.828 / 2,138.441 | 1.00× | 62.27 / 62.20 / 91.70 / 79.08 |
| records_array_1m / stringify | 175 | 2,196.571 / 2,199.143 / 843.577 / 968.177 | 1.00× | 61.58 / 60.62 / 106.27 / 100.22 |
| records_object_1m / parse | 71 | 4,033.085 / 4,003.127 / 2,925.944 / 2,139.394 | 1.01× | 80.47 / 80.39 / 91.64 / 75.89 |
| records_object_1m / stringify | 174 | 2,206.207 / 2,200.230 / 843.782 / 969.920 | 1.00× | 61.58 / 60.66 / 105.36 / 100.22 |
| records_array_8m / parse | 10 | 13,786.300 / 13,827.000 / 34,122.000 / 20,859.300 | 1.00× | 102.73 / 102.67 / 242.84 / 131.23 |
| records_array_8m / stringify | 20 | 18,676.400 / 18,693.100 / 6,752.050 / 8,469.700 | 1.00× | 196.00 / 195.92 / 184.88 / 169.72 |
| records_object_8m / parse | 7 | 31,815.571 / 31,477.000 / 29,715.714 / 20,537.000 | 1.01× | 213.48 / 213.45 / 209.12 / 106.50 |
| records_object_8m / stringify | 20 | 18,751.050 / 18,704.700 / 6,874.750 / 8,447.750 | 1.00× | 196.00 / 195.95 / 184.84 / 169.59 |
| records_array_20m / parse | 3 | 79,307.333 / 78,182.333 / 96,400.667 / 56,639.333 | 1.01× | 315.73 / 315.70 / 324.53 / 187.83 |
| records_array_20m / stringify | 8 | 97,367.875 / 97,767.875 / 17,436.250 / 20,853.625 | 1.00× | 176.16 / 176.09 / 391.03 / 304.73 |
| records_object_20m / parse | 3 | 79,253.000 / 78,502.667 / 94,339.000 / 56,769.667 | 1.01× | 315.72 / 315.70 / 324.72 / 187.84 |
| records_object_20m / stringify | 8 | 97,475.750 / 97,778.625 / 17,408.250 / 20,911.875 | 1.00× | 176.14 / 176.11 / 391.03 / 304.78 |
| numbers_1m / parse | 96 | 1,579.021 / 1,575.812 / 3,115.188 / 3,206.292 | 1.00× | 64.52 / 64.50 / 89.92 / 68.16 |
| numbers_1m / stringify | 75 | 6,602.640 / 6,600.413 / 1,931.600 / 2,953.747 | 1.00× | 61.44 / 61.34 / 80.69 / 70.19 |
| long_string_1m / parse | 1,343 | 208.328 / 205.574 / 368.820 / 67.511 | 1.01× | 62.92 / 60.81 / 159.73 / 164.36 |
| long_string_1m / stringify | 1,090 | 207.030 / 205.458 / 107.293 / 97.685 | 1.01× | 64.98 / 63.89 / 154.70 / 157.55 |
| escaped_1m / parse | 83 | 2,040.012 / 2,044.253 / 1,733.470 / 2,084.843 | 1.00× | 58.28 / 58.19 / 72.83 / 64.86 |
| escaped_1m / stringify | 83 | 1,861.205 / 1,867.663 / 1,856.108 / 2,091.771 | 1.00× | 57.03 / 56.97 / 78.48 / 68.95 |
| unicode_1m / parse | 1,512 | 983.136 / 788.621 / 437.574 / 60.475 | 1.25× | 56.42 / 55.50 / 164.70 / 157.94 |
| unicode_1m / stringify | 324 | 985.003 / 790.019 / 437.485 / 457.512 | 1.25× | 59.09 / 58.17 / 128.25 / 81.36 |
| wide_1m / parse | 36 | 21,028.361 / 20,779.167 / 4,985.944 / 4,162.361 | 1.01× | 176.81 / 176.72 / 109.39 / 86.09 |
| wide_1m / stringify | 215 | 2,498.340 / 2,486.493 / 6,357.126 / 665.967 | 1.00× | 70.97 / 70.89 / 115.27 / 84.50 |
| heterogeneous_1m / parse | 78 | 1,867.769 / 1,883.154 / 3,832.205 / 2,992.321 | 0.99× | 68.05 / 67.98 / 90.97 / 80.48 |
| heterogeneous_1m / stringify | 167 | 4,502.407 / 4,506.713 / 908.216 / 1,059.850 | 1.00× | 60.03 / 59.12 / 109.34 / 101.31 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.22 / 12.23 / 51.48 / 28.22 |
| tiny_object / retain-parse | 200,000 | 24.77 / 24.80 / 74.58 / 50.16 |
| tiny_object / retain-stringify | 1 | 12.47 / 12.41 / 51.58 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 36.34 / 36.34 / 71.12 / 46.97 |
| small_record / retain-parse | 1 | 12.25 / 12.27 / 51.53 / 28.25 |
| small_record / retain-parse | 100,000 | 56.98 / 56.97 / 85.31 / 52.62 |
| small_record / retain-stringify | 1 | 12.53 / 12.45 / 51.59 / 28.30 |
| small_record / retain-stringify | 100,000 | 31.31 / 31.27 / 80.23 / 51.00 |
| records_array_1m / retain-parse | 1 | 17.84 / 17.89 / 57.22 / 31.59 |
| records_array_1m / retain-parse | 16 | 41.25 / 41.30 / 87.50 / 55.84 |
| records_array_1m / retain-stringify | 1 | 20.03 / 20.02 / 59.36 / 34.39 |
| records_array_1m / retain-stringify | 16 | 33.94 / 33.91 / 75.22 / 47.19 |
| records_object_1m / retain-parse | 1 | 15.88 / 15.91 / 57.30 / 31.58 |
| records_object_1m / retain-parse | 16 | 67.05 / 67.03 / 87.56 / 55.86 |
| records_object_1m / retain-stringify | 1 | 20.08 / 20.03 / 59.34 / 34.41 |
| records_object_1m / retain-stringify | 16 | 33.98 / 33.94 / 75.19 / 47.19 |
| records_array_8m / retain-parse | 1 | 48.52 / 48.58 / 93.16 / 52.28 |
| records_array_8m / retain-parse | 4 | 89.22 / 89.31 / 145.75 / 91.78 |
| records_array_8m / retain-stringify | 1 | 75.66 / 75.64 / 113.03 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.95 / 94.94 / 142.72 / 94.44 |
| records_object_8m / retain-parse | 1 | 42.92 / 42.95 / 93.16 / 52.25 |
| records_object_8m / retain-parse | 4 | 113.44 / 113.45 / 145.72 / 91.39 |
| records_object_8m / retain-stringify | 1 | 75.78 / 75.78 / 112.97 / 73.41 |
| records_object_8m / retain-stringify | 4 | 95.08 / 95.08 / 142.80 / 94.42 |
| long_string_1m / retain-parse | 1 | 15.31 / 15.34 / 55.31 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.88 / 56.86 / 87.94 / 61.62 |
| long_string_1m / retain-stringify | 1 | 21.03 / 21.00 / 59.41 / 32.58 |
| long_string_1m / retain-stringify | 32 | 59.75 / 59.83 / 91.16 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.45 / 14.50 / 56.45 / 31.20 |
| unicode_1m / retain-parse | 32 | 44.67 / 44.72 / 85.09 / 58.91 |
| unicode_1m / retain-stringify | 1 | 18.58 / 18.55 / 59.61 / 34.45 |
| unicode_1m / retain-stringify | 32 | 48.53 / 48.50 / 87.98 / 62.17 |
| wide_1m / retain-parse | 1 | 24.08 / 24.09 / 64.48 / 36.27 |
| wide_1m / retain-parse | 16 | 71.27 / 71.28 / 111.31 / 68.83 |
| wide_1m / retain-stringify | 1 | 30.20 / 30.14 / 67.42 / 39.17 |
| wide_1m / retain-stringify | 16 | 45.47 / 45.41 / 85.30 / 53.41 |

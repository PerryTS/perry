Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.249 / 0.251 / 0.027 / 0.019 | 0.99× | 13.62 / 13.52 / 57.53 / 35.66 |
| null / stringify | 2,000,000 | 0.066 / 0.064 / 0.027 / 0.029 | 1.03× | 34.78 / 34.89 / 59.62 / 129.70 |
| string_a / parse | 2,000,000 | 0.253 / 0.255 / 0.032 / 0.023 | 0.99× | 13.62 / 13.52 / 57.56 / 36.05 |
| string_a / stringify | 2,000,000 | 0.073 / 0.072 / 0.030 / 0.029 | 1.02× | 34.75 / 34.88 / 59.52 / 129.70 |
| empty_object / parse | 2,000,000 | 0.324 / 0.326 / 0.045 / 0.024 | 1.00× | 34.55 / 34.64 / 59.50 / 69.03 |
| empty_object / stringify | 2,000,000 | 0.137 / 0.135 / 0.032 / 0.030 | 1.01× | 34.83 / 34.95 / 59.59 / 129.69 |
| tiny_object / parse | 2,000,000 | 0.366 / 0.364 / 0.082 / 0.045 | 1.01× | 34.55 / 34.66 / 59.50 / 68.98 |
| tiny_object / stringify | 2,000,000 | 0.196 / 0.197 / 0.037 / 0.040 | 1.00× | 34.86 / 34.95 / 59.56 / 129.69 |
| small_record / parse | 507,292 | 0.811 / 0.752 / 0.349 / 0.255 | 1.08× | 34.92 / 35.05 / 59.53 / 79.81 |
| small_record / stringify | 1,281,826 | 0.563 / 0.547 / 0.108 / 0.121 | 1.03× | 35.84 / 35.98 / 59.59 / 255.94 |
| object_1k / parse | 556,371 | 1.759 / 0.588 / 0.540 / 0.237 | 2.99× | 34.97 / 35.08 / 61.73 / 71.06 |
| object_1k / stringify | 648,473 | 1.135 / 0.385 / 0.199 / 0.215 | 2.95× | 36.02 / 36.11 / 61.70 / 70.73 |
| records_array_16k / parse | 6,678 | 30.394 / 23.798 / 39.704 / 33.970 | 1.28× | 64.91 / 65.03 / 65.73 / 70.39 |
| records_array_16k / stringify | 9,600 | 37.634 / 35.891 / 13.226 / 23.224 | 1.05× | 36.19 / 36.28 / 61.67 / 70.97 |
| records_array_1m / parse | 91 | 1,971.033 / 1,542.044 / 2,882.154 / 2,143.923 | 1.28× | 62.20 / 62.31 / 92.83 / 79.12 |
| records_array_1m / stringify | 177 | 2,358.068 / 2,226.379 / 843.582 / 968.209 | 1.06× | 60.42 / 61.36 / 109.25 / 100.27 |
| records_object_1m / parse | 71 | 4,547.789 / 4,027.732 / 2,792.169 / 2,132.577 | 1.13× | 80.06 / 80.16 / 92.95 / 75.89 |
| records_object_1m / stringify | 177 | 2,362.661 / 2,224.395 / 842.254 / 970.328 | 1.06× | 60.44 / 61.33 / 109.16 / 100.23 |
| records_array_8m / parse | 10 | 17,148.100 / 13,814.300 / 33,726.900 / 20,769.600 | 1.24× | 102.64 / 102.75 / 243.95 / 131.61 |
| records_array_8m / stringify | 20 | 19,858.400 / 18,854.750 / 6,823.450 / 8,439.550 | 1.05× | 196.00 / 196.12 / 186.12 / 169.73 |
| records_object_8m / parse | 8 | 35,049.875 / 31,341.125 / 30,315.625 / 21,791.250 | 1.12× | 231.81 / 231.80 / 225.69 / 111.33 |
| records_object_8m / stringify | 20 | 19,859.450 / 18,828.900 / 6,785.450 / 8,463.400 | 1.05× | 196.03 / 196.11 / 185.88 / 169.84 |
| records_array_20m / parse | 3 | 87,776.333 / 78,684.667 / 96,424.667 / 56,900.667 | 1.12× | 315.83 / 315.78 / 326.22 / 188.12 |
| records_array_20m / stringify | 8 | 101,772.500 / 97,970.875 / 17,327.750 / 20,780.125 | 1.04× | 171.33 / 171.38 / 392.17 / 304.77 |
| records_object_20m / parse | 3 | 87,357.000 / 78,275.667 / 95,968.000 / 56,588.667 | 1.12× | 315.83 / 315.78 / 326.22 / 188.03 |
| records_object_20m / stringify | 8 | 101,723.625 / 98,155.125 / 17,326.500 / 20,877.250 | 1.04× | 171.33 / 171.36 / 392.17 / 304.86 |
| numbers_1m / parse | 96 | 2,265.333 / 1,575.042 / 3,121.271 / 3,209.760 | 1.44× | 64.47 / 64.55 / 91.27 / 68.16 |
| numbers_1m / stringify | 76 | 7,906.566 / 7,918.434 / 1,931.303 / 2,951.000 | 1.00× | 60.36 / 60.50 / 81.91 / 70.22 |
| long_string_1m / parse | 1,053 | 1,440.082 / 204.024 / 370.518 / 68.654 | 7.06× | 60.81 / 62.95 / 151.83 / 153.34 |
| long_string_1m / stringify | 1,058 | 980.655 / 206.698 / 107.323 / 97.802 | 4.74× | 63.92 / 65.05 / 154.91 / 155.55 |
| escaped_1m / parse | 83 | 2,612.627 / 2,038.229 / 1,733.699 / 2,083.964 | 1.28× | 58.20 / 58.31 / 74.00 / 64.86 |
| escaped_1m / stringify | 82 | 1,903.927 / 1,904.159 / 1,851.659 / 2,095.256 | 1.00× | 57.02 / 57.09 / 79.56 / 68.97 |
| unicode_1m / parse | 750 | 2,039.944 / 984.212 / 445.644 / 60.616 | 2.07× | 55.45 / 56.45 / 146.59 / 98.11 |
| unicode_1m / stringify | 322 | 1,642.053 / 986.531 / 437.335 / 457.618 | 1.66× | 58.22 / 59.17 / 129.45 / 81.38 |
| wide_1m / parse | 3 | 405,493.000 / 10,801.000 / 5,586.000 / 4,123.000 | 37.54× | 38.52 / 41.17 / 79.86 / 45.84 |
| wide_1m / stringify | 214 | 2,767.949 / 2,507.533 / 6,339.822 / 665.150 | 1.10× | 71.03 / 70.70 / 116.42 / 84.62 |
| heterogeneous_1m / parse | 78 | 2,159.141 / 1,867.808 / 3,956.833 / 2,964.397 | 1.16× | 68.06 / 68.20 / 92.19 / 80.70 |
| heterogeneous_1m / stringify | 168 | 4,457.988 / 4,511.310 / 905.810 / 1,061.095 | 0.99× | 59.19 / 60.11 / 110.77 / 101.48 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.34 / 12.28 / 52.64 / 28.23 |
| tiny_object / retain-parse | 200,000 | 24.91 / 24.83 / 74.73 / 50.16 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.55 / 52.72 / 28.27 |
| tiny_object / retain-stringify | 200,000 | 36.34 / 36.39 / 71.22 / 46.98 |
| small_record / retain-parse | 1 | 12.36 / 12.30 / 52.62 / 28.23 |
| small_record / retain-parse | 100,000 | 56.94 / 56.98 / 85.41 / 52.64 |
| small_record / retain-stringify | 1 | 12.61 / 12.58 / 52.77 / 28.28 |
| small_record / retain-stringify | 100,000 | 31.42 / 31.36 / 80.38 / 51.02 |
| records_array_1m / retain-parse | 1 | 17.98 / 17.91 / 58.30 / 31.58 |
| records_array_1m / retain-parse | 16 | 41.39 / 41.33 / 88.70 / 55.88 |
| records_array_1m / retain-stringify | 1 | 20.19 / 20.12 / 60.44 / 34.39 |
| records_array_1m / retain-stringify | 16 | 34.08 / 34.02 / 76.34 / 47.20 |
| records_object_1m / retain-parse | 1 | 16.02 / 15.94 / 58.36 / 31.58 |
| records_object_1m / retain-parse | 16 | 67.03 / 67.17 / 88.69 / 55.86 |
| records_object_1m / retain-stringify | 1 | 20.19 / 20.16 / 60.48 / 34.41 |
| records_object_1m / retain-stringify | 16 | 34.08 / 34.05 / 76.33 / 47.20 |
| records_array_8m / retain-parse | 1 | 48.64 / 48.55 / 94.25 / 52.27 |
| records_array_8m / retain-parse | 4 | 89.19 / 89.28 / 146.89 / 92.02 |
| records_array_8m / retain-stringify | 1 | 75.70 / 75.78 / 114.19 / 73.42 |
| records_array_8m / retain-stringify | 4 | 95.00 / 95.08 / 143.92 / 94.42 |
| records_object_8m / retain-parse | 1 | 42.97 / 42.95 / 94.34 / 52.28 |
| records_object_8m / retain-parse | 4 | 113.39 / 113.47 / 146.92 / 91.83 |
| records_object_8m / retain-stringify | 1 | 75.72 / 75.78 / 114.17 / 73.41 |
| records_object_8m / retain-stringify | 4 | 95.02 / 95.08 / 143.95 / 94.44 |
| long_string_1m / retain-parse | 1 | 15.41 / 15.39 / 56.52 / 30.58 |
| long_string_1m / retain-parse | 32 | 56.80 / 56.97 / 89.16 / 61.62 |
| long_string_1m / retain-stringify | 1 | 21.08 / 21.08 / 60.56 / 32.58 |
| long_string_1m / retain-stringify | 32 | 59.78 / 59.91 / 92.23 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.59 / 14.55 / 57.61 / 31.27 |
| unicode_1m / retain-parse | 32 | 44.80 / 44.73 / 86.27 / 58.91 |
| unicode_1m / retain-stringify | 1 | 18.66 / 18.64 / 60.84 / 34.05 |
| unicode_1m / retain-stringify | 32 | 48.62 / 48.59 / 89.11 / 61.70 |
| wide_1m / retain-parse | 1 | 22.88 / 24.53 / 65.56 / 36.27 |
| wide_1m / retain-parse | 16 | 73.61 / 75.62 / 112.48 / 68.89 |
| wide_1m / retain-stringify | 1 | 29.47 / 30.64 / 68.62 / 39.20 |
| wide_1m / retain-stringify | 16 | 44.73 / 45.92 / 86.53 / 53.42 |

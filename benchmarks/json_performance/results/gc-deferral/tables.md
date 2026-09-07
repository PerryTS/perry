Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.47 / 13.55 / 57.53 / 35.70 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.62 / 13.64 / 59.44 / 129.75 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.47 / 13.53 / 57.59 / 36.06 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.61 / 13.62 / 59.53 / 129.80 |
| empty_object / parse | 2,000,000 | 0.057 / 0.057 / 0.045 / 0.024 | 1.00× | 34.78 / 34.75 / 59.42 / 69.05 |
| empty_object / stringify | 2,000,000 | 0.047 / 0.047 / 0.032 / 0.030 | 0.99× | 13.78 / 13.73 / 59.50 / 129.78 |
| tiny_object / parse | 2,000,000 | 0.122 / 0.122 / 0.082 / 0.045 | 1.00× | 34.80 / 34.75 / 59.44 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.093 / 0.093 / 0.037 / 0.039 | 0.99× | 34.83 / 34.77 / 59.62 / 129.78 |
| small_record / parse | 574,850 | 0.543 / 0.540 / 0.356 / 0.253 | 1.01× | 34.55 / 34.50 / 59.55 / 79.86 |
| small_record / stringify | 1,228,249 | 0.284 / 0.284 / 0.109 / 0.121 | 1.00× | 35.98 / 35.92 / 59.61 / 246.84 |
| object_1k / parse | 562,411 | 0.542 / 0.541 / 0.543 / 0.237 | 1.00× | 35.00 / 34.97 / 61.69 / 71.09 |
| object_1k / stringify | 649,702 | 0.237 / 0.237 / 0.199 / 0.215 | 1.00× | 35.94 / 35.89 / 61.72 / 70.81 |
| records_array_16k / parse | 6,672 | 23.835 / 23.877 / 39.775 / 33.940 | 1.00× | 64.83 / 64.88 / 65.78 / 70.41 |
| records_array_16k / stringify | 9,836 | 23.591 / 23.404 / 13.197 / 23.217 | 1.01× | 36.27 / 36.12 / 61.67 / 71.00 |
| records_array_1m / parse | 95 | 1,557.916 / 1,551.368 / 2,719.074 / 2,133.695 | 1.00× | 62.06 / 62.09 / 92.86 / 79.14 |
| records_array_1m / stringify | 177 | 1,496.814 / 1,507.972 / 843.316 / 968.763 | 0.99× | 59.41 / 59.33 / 109.00 / 100.30 |
| records_object_1m / parse | 71 | 2,835.042 / 2,823.042 / 2,773.930 / 2,132.944 | 1.00× | 60.23 / 58.62 / 92.75 / 75.91 |
| records_object_1m / stringify | 174 | 1,495.994 / 1,511.236 / 841.644 / 971.977 | 0.99× | 59.44 / 59.34 / 106.67 / 100.31 |
| records_array_8m / parse | 10 | 13,387.800 / 13,404.900 / 35,203.900 / 20,820.200 | 1.00× | 99.47 / 100.02 / 243.84 / 131.22 |
| records_array_8m / stringify | 20 | 12,962.650 / 13,028.150 / 6,791.800 / 8,454.700 | 0.99× | 193.72 / 193.70 / 186.00 / 169.77 |
| records_object_8m / parse | 8 | 21,609.125 / 21,483.500 / 29,457.250 / 21,760.375 | 1.01× | 166.88 / 166.75 / 225.53 / 111.83 |
| records_object_8m / stringify | 21 | 12,869.762 / 12,988.476 / 6,808.095 / 8,471.905 | 0.99× | 193.77 / 193.73 / 185.97 / 184.38 |
| records_array_20m / parse | 3 | 53,341.333 / 52,899.000 / 96,497.667 / 56,723.000 | 1.01× | 224.30 / 224.22 / 326.06 / 188.30 |
| records_array_20m / stringify | 8 | 81,785.000 / 82,190.000 / 17,363.125 / 20,818.750 | 1.00× | 158.78 / 158.69 / 392.12 / 304.73 |
| records_object_20m / parse | 3 | 53,354.667 / 52,904.667 / 96,428.000 / 56,561.667 | 1.01× | 224.30 / 224.22 / 325.95 / 188.25 |
| records_object_20m / stringify | 8 | 82,051.500 / 82,050.625 / 17,303.875 / 20,925.250 | 1.00× | 158.84 / 158.73 / 392.03 / 304.77 |
| numbers_1m / parse | 92 | 1,738.228 / 1,715.837 / 3,125.250 / 3,198.913 | 1.01× | 64.88 / 64.94 / 87.31 / 68.20 |
| numbers_1m / stringify | 96 | 1,609.646 / 1,604.010 / 1,980.802 / 2,956.365 | 1.00× | 63.47 / 63.44 / 92.25 / 72.27 |
| long_string_1m / parse | 1,308 | 204.501 / 203.463 / 368.655 / 66.021 | 1.01× | 60.73 / 60.77 / 159.75 / 124.41 |
| long_string_1m / stringify | 1,130 | 173.852 / 174.393 / 105.877 / 97.797 | 1.00× | 61.94 / 61.97 / 156.81 / 159.61 |
| escaped_1m / parse | 82 | 2,039.939 / 2,041.280 / 1,733.683 / 2,085.354 | 1.00× | 58.12 / 58.14 / 73.88 / 64.17 |
| escaped_1m / stringify | 168 | 951.685 / 951.685 / 1,894.958 / 2,086.482 | 1.00× | 57.16 / 57.17 / 124.64 / 74.77 |
| unicode_1m / parse | 1,520 | 258.484 / 257.013 / 437.687 / 60.661 | 1.01× | 55.44 / 55.48 / 166.78 / 156.22 |
| unicode_1m / stringify | 1,408 | 139.892 / 139.662 / 412.677 / 450.919 | 1.00× | 57.17 / 57.22 / 157.67 / 126.27 |
| wide_1m / parse | 36 | 20,399.056 / 15,180.139 / 4,920.000 / 4,166.333 | 1.34× | 176.66 / 167.44 / 110.61 / 86.12 |
| wide_1m / stringify | 215 | 1,692.874 / 1,708.209 / 6,337.674 / 666.205 | 0.99× | 70.98 / 70.94 / 116.36 / 83.27 |
| heterogeneous_1m / parse | 80 | 1,935.338 / 1,935.612 / 3,886.088 / 2,953.262 | 1.00× | 69.34 / 69.38 / 92.14 / 80.72 |
| heterogeneous_1m / stringify | 211 | 848.109 / 848.720 / 902.673 / 1,053.900 | 1.00× | 59.22 / 59.19 / 128.69 / 103.09 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.14 / 12.20 / 52.58 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.70 / 24.77 / 74.70 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.30 / 12.34 / 52.72 / 28.31 |
| tiny_object / retain-stringify | 200,000 | 24.84 / 24.91 / 71.20 / 47.03 |
| small_record / retain-parse | 1 | 12.17 / 12.23 / 52.61 / 28.28 |
| small_record / retain-parse | 100,000 | 44.91 / 44.83 / 85.44 / 52.67 |
| small_record / retain-stringify | 1 | 12.38 / 12.41 / 52.73 / 28.30 |
| small_record / retain-stringify | 100,000 | 28.06 / 28.11 / 80.31 / 51.05 |
| records_array_1m / retain-parse | 1 | 16.33 / 16.38 / 58.27 / 31.62 |
| records_array_1m / retain-parse | 16 | 41.53 / 41.58 / 88.70 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.28 / 19.22 / 60.44 / 34.48 |
| records_array_1m / retain-stringify | 16 | 33.16 / 33.11 / 76.30 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.00 / 15.06 / 58.41 / 31.62 |
| records_object_1m / retain-parse | 16 | 56.53 / 57.23 / 88.73 / 55.94 |
| records_object_1m / retain-stringify | 1 | 19.28 / 19.25 / 60.48 / 34.47 |
| records_object_1m / retain-stringify | 16 | 33.19 / 33.14 / 76.28 / 47.25 |
| records_array_8m / retain-parse | 1 | 48.00 / 48.02 / 94.27 / 52.33 |
| records_array_8m / retain-parse | 4 | 86.31 / 86.34 / 146.92 / 91.84 |
| records_array_8m / retain-stringify | 1 | 72.84 / 72.77 / 114.12 / 73.47 |
| records_array_8m / retain-stringify | 4 | 91.58 / 91.50 / 144.05 / 94.48 |
| records_object_8m / retain-parse | 1 | 36.12 / 36.23 / 94.25 / 52.31 |
| records_object_8m / retain-parse | 4 | 97.42 / 97.42 / 146.89 / 91.94 |
| records_object_8m / retain-stringify | 1 | 72.89 / 72.80 / 114.19 / 73.45 |
| records_object_8m / retain-stringify | 4 | 91.62 / 91.53 / 143.88 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.12 / 15.33 / 56.45 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.67 / 56.73 / 89.14 / 61.62 |
| long_string_1m / retain-stringify | 1 | 18.67 / 18.78 / 60.48 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.67 / 58.72 / 92.27 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.41 / 14.48 / 57.62 / 31.31 |
| unicode_1m / retain-parse | 32 | 44.61 / 44.69 / 86.30 / 58.95 |
| unicode_1m / retain-stringify | 1 | 16.67 / 16.75 / 60.78 / 34.03 |
| unicode_1m / retain-stringify | 32 | 46.62 / 46.70 / 89.14 / 61.78 |
| wide_1m / retain-parse | 1 | 24.00 / 24.09 / 65.56 / 36.28 |
| wide_1m / retain-parse | 16 | 71.17 / 71.27 / 112.50 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.19 / 30.30 / 68.56 / 39.25 |
| wide_1m / retain-stringify | 16 | 45.45 / 45.47 / 86.45 / 53.47 |

Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.251 / 0.247 / 0.027 / 0.019 | 1.01× | 13.55 / 13.66 / 57.53 / 35.66 |
| null / stringify | 2,000,000 | 0.066 / 0.007 / 0.027 / 0.029 | 10.08× | 34.89 / 13.73 / 59.52 / 129.73 |
| string_a / parse | 2,000,000 | 0.254 / 0.253 / 0.032 / 0.023 | 1.01× | 13.55 / 13.66 / 57.55 / 36.06 |
| string_a / stringify | 2,000,000 | 0.075 / 0.010 / 0.030 / 0.029 | 7.70× | 34.91 / 13.73 / 59.48 / 129.70 |
| empty_object / parse | 2,000,000 | 0.326 / 0.321 / 0.045 / 0.025 | 1.02× | 34.64 / 34.73 / 59.47 / 69.05 |
| empty_object / stringify | 2,000,000 | 0.136 / 0.051 / 0.032 / 0.030 | 2.66× | 34.92 / 13.78 / 59.56 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.364 / 0.361 / 0.081 / 0.045 | 1.01× | 34.62 / 34.73 / 59.59 / 69.05 |
| tiny_object / stringify | 2,000,000 | 0.197 / 0.140 / 0.037 / 0.040 | 1.41× | 34.92 / 34.97 / 59.52 / 129.75 |
| small_record / parse | 576,969 | 0.751 / 0.745 / 0.377 / 0.255 | 1.01× | 35.02 / 35.09 / 59.48 / 79.84 |
| small_record / stringify | 1,262,934 | 0.550 / 0.549 / 0.109 / 0.121 | 1.00× | 35.98 / 36.03 / 59.61 / 252.75 |
| object_1k / parse | 540,540 | 0.591 / 0.588 / 0.541 / 0.238 | 1.00× | 35.06 / 35.16 / 61.70 / 71.08 |
| object_1k / stringify | 646,551 | 0.385 / 0.292 / 0.199 / 0.216 | 1.32× | 36.06 / 36.09 / 61.70 / 70.81 |
| records_array_16k / parse | 6,654 | 23.892 / 23.902 / 39.742 / 34.083 | 1.00× | 65.02 / 65.08 / 65.69 / 70.45 |
| records_array_16k / stringify | 9,561 | 35.725 / 32.013 / 13.845 / 23.285 | 1.12× | 36.36 / 36.39 / 61.75 / 70.98 |
| records_array_1m / parse | 93 | 1,541.817 / 1,524.699 / 2,884.215 / 2,146.892 | 1.01× | 62.28 / 62.36 / 92.73 / 79.09 |
| records_array_1m / stringify | 175 | 2,202.360 / 1,988.309 / 856.377 / 974.829 | 1.11× | 60.77 / 60.78 / 107.52 / 100.28 |
| records_object_1m / parse | 70 | 4,042.500 / 4,015.171 / 2,901.700 / 2,154.614 | 1.01× | 80.42 / 80.52 / 92.78 / 75.92 |
| records_object_1m / stringify | 173 | 2,215.191 / 1,985.185 / 857.572 / 976.769 | 1.12× | 60.77 / 60.80 / 105.75 / 100.28 |
| records_array_8m / parse | 10 | 13,890.000 / 13,725.600 / 34,039.900 / 21,145.400 | 1.01× | 102.75 / 102.83 / 244.00 / 131.05 |
| records_array_8m / stringify | 20 | 18,743.150 / 16,858.350 / 6,847.000 / 8,578.600 | 1.11× | 195.98 / 196.08 / 186.06 / 189.73 |
| records_object_8m / parse | 8 | 31,909.250 / 31,692.375 / 29,430.125 / 22,010.750 | 1.01× | 231.70 / 231.80 / 225.47 / 111.59 |
| records_object_8m / stringify | 20 | 18,734.300 / 16,852.800 / 6,849.350 / 8,452.250 | 1.11× | 195.98 / 196.09 / 186.02 / 169.62 |
| records_array_20m / parse | 3 | 78,776.333 / 78,341.333 / 95,114.333 / 57,376.667 | 1.01× | 315.69 / 315.77 / 325.73 / 187.89 |
| records_array_20m / stringify | 8 | 97,416.500 / 93,936.875 / 17,384.375 / 20,929.250 | 1.04× | 176.16 / 176.23 / 392.17 / 304.78 |
| records_object_20m / parse | 3 | 78,742.667 / 78,349.333 / 93,463.667 / 57,632.333 | 1.01× | 315.83 / 315.88 / 325.92 / 187.86 |
| records_object_20m / stringify | 8 | 97,590.500 / 94,041.875 / 17,491.250 / 20,882.125 | 1.04× | 171.36 / 171.39 / 392.16 / 304.89 |
| numbers_1m / parse | 95 | 1,566.305 / 1,717.537 / 3,126.989 / 3,207.905 | 0.91× | 64.55 / 64.67 / 90.27 / 68.14 |
| numbers_1m / stringify | 96 | 6,608.885 / 1,605.323 / 1,982.292 / 2,952.812 | 4.12× | 61.44 / 61.47 / 92.09 / 72.20 |
| long_string_1m / parse | 1,284 | 203.184 / 203.782 / 370.551 / 68.311 | 1.00× | 60.92 / 61.00 / 158.92 / 153.39 |
| long_string_1m / stringify | 1,139 | 206.153 / 172.542 / 106.516 / 96.737 | 1.19× | 64.02 / 62.17 / 157.89 / 123.59 |
| escaped_1m / parse | 82 | 2,042.305 / 2,043.317 / 1,733.537 / 2,088.463 | 1.00× | 58.30 / 58.38 / 73.89 / 64.16 |
| escaped_1m / stringify | 83 | 1,864.181 / 1,907.434 / 1,865.470 / 2,099.735 | 0.98× | 57.08 / 57.17 / 79.47 / 68.97 |
| unicode_1m / parse | 1,394 | 256.184 / 255.900 / 439.301 / 61.500 | 1.00× | 55.64 / 55.67 / 163.11 / 157.92 |
| unicode_1m / stringify | 1,425 | 255.476 / 138.107 / 414.983 / 453.283 | 1.85× | 58.41 / 57.41 / 158.53 / 155.73 |
| wide_1m / parse | 36 | 21,354.278 / 20,807.972 / 4,955.778 / 4,186.472 | 1.03× | 176.77 / 176.92 / 110.48 / 86.11 |
| wide_1m / stringify | 213 | 2,476.315 / 2,479.920 / 6,350.944 / 670.408 | 1.00× | 71.00 / 71.05 / 116.42 / 84.64 |
| heterogeneous_1m / parse | 78 | 1,871.949 / 1,918.705 / 3,812.192 / 2,971.641 | 0.98× | 68.06 / 68.14 / 92.19 / 80.70 |
| heterogeneous_1m / stringify | 162 | 4,490.432 / 4,522.049 / 906.772 / 1,070.728 | 0.99× | 59.27 / 59.30 / 110.41 / 101.41 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.33 / 12.41 / 52.61 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.91 / 25.02 / 74.70 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.56 / 52.73 / 28.34 |
| tiny_object / retain-stringify | 200,000 | 36.41 / 25.16 / 71.22 / 47.03 |
| small_record / retain-parse | 1 | 12.36 / 12.42 / 52.61 / 28.30 |
| small_record / retain-parse | 100,000 | 57.02 / 57.03 / 85.50 / 52.67 |
| small_record / retain-stringify | 1 | 12.61 / 12.64 / 52.77 / 28.31 |
| small_record / retain-stringify | 100,000 | 31.44 / 31.47 / 80.30 / 51.05 |
| records_array_1m / retain-parse | 1 | 17.95 / 18.02 / 58.44 / 31.69 |
| records_array_1m / retain-parse | 16 | 41.38 / 41.44 / 88.64 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.16 / 20.20 / 60.53 / 34.45 |
| records_array_1m / retain-stringify | 16 | 34.05 / 34.09 / 76.38 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.98 / 16.09 / 58.41 / 31.61 |
| records_object_1m / retain-parse | 16 | 67.09 / 67.12 / 88.66 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.17 / 20.22 / 60.59 / 34.42 |
| records_object_1m / retain-stringify | 16 | 34.06 / 34.09 / 76.41 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.64 / 48.70 / 94.27 / 52.28 |
| records_array_8m / retain-parse | 4 | 89.33 / 89.45 / 146.92 / 91.97 |
| records_array_8m / retain-stringify | 1 | 75.70 / 75.67 / 114.16 / 73.45 |
| records_array_8m / retain-stringify | 4 | 95.00 / 94.97 / 143.97 / 94.53 |
| records_object_8m / retain-parse | 1 | 43.05 / 43.16 / 94.23 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.41 / 113.50 / 146.89 / 91.72 |
| records_object_8m / retain-stringify | 1 | 75.81 / 75.80 / 114.16 / 73.44 |
| records_object_8m / retain-stringify | 4 | 95.11 / 95.09 / 143.66 / 94.56 |
| long_string_1m / retain-parse | 1 | 15.44 / 15.53 / 56.48 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.94 / 57.08 / 89.09 / 61.64 |
| long_string_1m / retain-stringify | 1 | 21.16 / 19.03 / 60.55 / 32.58 |
| long_string_1m / retain-stringify | 32 | 59.91 / 59.03 / 92.23 / 63.70 |
| unicode_1m / retain-parse | 1 | 14.64 / 14.72 / 57.66 / 31.23 |
| unicode_1m / retain-parse | 32 | 44.84 / 44.92 / 86.23 / 58.92 |
| unicode_1m / retain-stringify | 1 | 18.73 / 16.97 / 60.86 / 34.03 |
| unicode_1m / retain-stringify | 32 | 48.69 / 46.94 / 89.17 / 61.73 |
| wide_1m / retain-parse | 1 | 24.20 / 24.31 / 65.50 / 36.27 |
| wide_1m / retain-parse | 16 | 71.38 / 71.48 / 112.44 / 68.84 |
| wide_1m / retain-stringify | 1 | 30.28 / 30.36 / 68.59 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.55 / 45.62 / 86.48 / 53.45 |

Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.251 / 0.246 / 0.027 / 0.019 | 1.02× | 13.55 / 13.47 / 57.53 / 35.69 |
| null / stringify | 2,000,000 | 0.066 / 0.007 / 0.027 / 0.029 | 10.03× | 34.89 / 13.56 / 59.55 / 129.75 |
| string_a / parse | 2,000,000 | 0.254 / 0.250 / 0.032 / 0.023 | 1.02× | 13.55 / 13.47 / 57.53 / 36.08 |
| string_a / stringify | 2,000,000 | 0.074 / 0.010 / 0.030 / 0.029 | 7.68× | 34.86 / 13.55 / 59.55 / 129.77 |
| empty_object / parse | 2,000,000 | 0.325 / 0.326 / 0.045 / 0.024 | 1.00× | 34.64 / 34.55 / 59.53 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.136 / 0.052 / 0.032 / 0.030 | 2.62× | 34.92 / 13.58 / 59.62 / 129.77 |
| tiny_object / parse | 2,000,000 | 0.363 / 0.365 / 0.082 / 0.045 | 1.00× | 34.64 / 34.55 / 59.48 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.197 / 0.142 / 0.037 / 0.039 | 1.38× | 34.92 / 34.78 / 59.53 / 129.73 |
| small_record / parse | 580,457 | 0.750 / 0.744 / 0.340 / 0.253 | 1.01× | 35.02 / 34.97 / 59.58 / 79.86 |
| small_record / stringify | 1,282,736 | 0.550 / 0.547 / 0.109 / 0.121 | 1.00× | 35.98 / 35.97 / 59.67 / 256.16 |
| object_1k / parse | 562,236 | 0.592 / 0.588 / 0.540 / 0.237 | 1.01× | 35.05 / 35.00 / 61.69 / 71.11 |
| object_1k / stringify | 640,454 | 0.387 / 0.292 / 0.200 / 0.215 | 1.32× | 36.08 / 35.92 / 61.73 / 70.80 |
| records_array_16k / parse | 6,728 | 23.777 / 23.892 / 40.139 / 33.909 | 1.00× | 65.02 / 64.80 / 65.73 / 70.44 |
| records_array_16k / stringify | 10,155 | 35.822 / 31.532 / 13.286 / 23.212 | 1.14× | 36.28 / 36.22 / 61.78 / 71.03 |
| records_array_1m / parse | 93 | 1,542.989 / 1,572.624 / 2,761.624 / 2,133.129 | 0.98× | 62.31 / 62.06 / 92.88 / 79.12 |
| records_array_1m / stringify | 176 | 2,202.898 / 1,971.432 / 848.307 / 969.386 | 1.12× | 60.56 / 61.19 / 108.34 / 100.30 |
| records_object_1m / parse | 70 | 4,027.657 / 4,011.200 / 2,786.729 / 2,134.314 | 1.00× | 80.14 / 80.08 / 92.66 / 75.88 |
| records_object_1m / stringify | 178 | 2,204.511 / 1,982.292 / 850.753 / 970.129 | 1.11× | 60.55 / 61.16 / 110.14 / 100.25 |
| records_array_8m / parse | 10 | 13,781.000 / 14,084.700 / 34,222.900 / 20,836.200 | 0.98× | 102.75 / 102.52 / 244.05 / 131.22 |
| records_array_8m / stringify | 20 | 18,721.150 / 16,817.100 / 6,867.500 / 8,442.950 | 1.11× | 196.03 / 196.02 / 186.03 / 169.59 |
| records_object_8m / parse | 8 | 31,435.875 / 31,226.000 / 30,687.875 / 21,756.250 | 1.01× | 231.80 / 231.78 / 225.58 / 111.52 |
| records_object_8m / stringify | 20 | 18,696.200 / 16,939.700 / 6,824.850 / 8,431.250 | 1.10× | 196.05 / 196.00 / 185.94 / 169.48 |
| records_array_20m / parse | 3 | 78,332.667 / 77,707.667 / 95,292.000 / 56,441.333 | 1.01× | 315.81 / 315.78 / 325.86 / 187.98 |
| records_array_20m / stringify | 8 | 97,460.000 / 94,405.875 / 17,321.875 / 20,790.875 | 1.03× | 171.38 / 171.25 / 392.17 / 304.88 |
| records_object_20m / parse | 3 | 78,189.667 / 77,281.000 / 96,488.333 / 56,847.000 | 1.01× | 315.81 / 315.78 / 326.02 / 187.88 |
| records_object_20m / stringify | 8 | 97,740.875 / 94,219.875 / 17,378.875 / 20,956.500 | 1.04× | 171.36 / 171.11 / 392.08 / 304.88 |
| numbers_1m / parse | 97 | 1,571.948 / 1,854.278 / 3,129.113 / 3,197.299 | 0.85× | 64.53 / 64.38 / 92.33 / 68.16 |
| numbers_1m / stringify | 94 | 6,608.277 / 1,640.989 / 1,983.468 / 2,953.053 | 4.03× | 61.45 / 61.31 / 90.19 / 72.22 |
| long_string_1m / parse | 1,203 | 204.485 / 206.962 / 369.602 / 67.694 | 0.99× | 60.89 / 62.72 / 156.77 / 153.39 |
| long_string_1m / stringify | 1,074 | 205.316 / 173.356 / 112.158 / 97.980 | 1.18× | 64.00 / 64.00 / 154.91 / 157.58 |
| escaped_1m / parse | 82 | 2,042.671 / 2,040.902 / 1,732.866 / 2,084.878 | 1.00× | 58.30 / 58.11 / 74.05 / 64.17 |
| escaped_1m / stringify | 83 | 1,861.940 / 1,904.904 / 1,851.602 / 2,087.048 | 0.98× | 57.05 / 56.98 / 79.61 / 68.97 |
| unicode_1m / parse | 1,473 | 257.261 / 258.680 / 437.948 / 60.251 | 0.99× | 55.64 / 56.31 / 164.92 / 156.12 |
| unicode_1m / stringify | 1,398 | 254.841 / 137.391 / 412.891 / 449.829 | 1.85× | 58.39 / 58.00 / 157.61 / 131.16 |
| wide_1m / parse | 36 | 21,015.972 / 21,010.222 / 4,958.917 / 4,147.361 | 1.00× | 176.77 / 176.66 / 110.53 / 86.12 |
| wide_1m / stringify | 215 | 2,470.995 / 2,462.647 / 6,359.353 / 665.623 | 1.00× | 71.00 / 72.62 / 116.48 / 84.50 |
| heterogeneous_1m / parse | 78 | 1,868.846 / 1,930.756 / 3,851.346 / 2,956.769 | 0.97× | 68.19 / 67.97 / 92.22 / 80.89 |
| heterogeneous_1m / stringify | 166 | 4,502.229 / 4,559.127 / 907.247 / 1,060.687 | 0.99× | 59.27 / 59.92 / 110.56 / 101.34 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.33 / 12.19 / 52.62 / 28.25 |
| tiny_object / retain-parse | 200,000 | 24.91 / 24.77 / 74.81 / 50.17 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.36 / 52.78 / 28.28 |
| tiny_object / retain-stringify | 200,000 | 36.39 / 24.92 / 71.23 / 47.02 |
| small_record / retain-parse | 1 | 12.34 / 12.25 / 52.70 / 28.27 |
| small_record / retain-parse | 100,000 | 57.00 / 56.94 / 85.42 / 52.67 |
| small_record / retain-stringify | 1 | 12.61 / 12.55 / 52.72 / 28.30 |
| small_record / retain-stringify | 100,000 | 31.42 / 31.38 / 80.33 / 51.06 |
| records_array_1m / retain-parse | 1 | 17.97 / 17.84 / 58.47 / 31.61 |
| records_array_1m / retain-parse | 16 | 41.38 / 41.27 / 88.61 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.19 / 20.08 / 60.50 / 34.42 |
| records_array_1m / retain-stringify | 16 | 34.06 / 33.95 / 76.42 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.98 / 15.91 / 58.34 / 31.59 |
| records_object_1m / retain-parse | 16 | 67.14 / 67.08 / 88.70 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.17 / 20.08 / 60.50 / 34.47 |
| records_object_1m / retain-stringify | 16 | 34.06 / 33.98 / 76.45 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.62 / 48.48 / 94.36 / 52.30 |
| records_array_8m / retain-parse | 4 | 89.30 / 89.08 / 146.89 / 91.91 |
| records_array_8m / retain-stringify | 1 | 75.77 / 75.70 / 114.14 / 73.45 |
| records_array_8m / retain-stringify | 4 | 95.06 / 95.00 / 143.92 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.94 / 42.77 / 94.28 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.44 / 113.41 / 146.84 / 91.91 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.72 / 114.25 / 73.44 |
| records_object_8m / retain-stringify | 4 | 95.06 / 95.02 / 143.89 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.42 / 15.34 / 56.52 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.94 / 56.78 / 89.11 / 61.61 |
| long_string_1m / retain-stringify | 1 | 21.09 / 18.78 / 60.50 / 32.58 |
| long_string_1m / retain-stringify | 32 | 59.89 / 58.73 / 92.27 / 63.70 |
| unicode_1m / retain-parse | 1 | 14.62 / 14.53 / 57.62 / 31.30 |
| unicode_1m / retain-parse | 32 | 44.84 / 44.72 / 86.27 / 58.94 |
| unicode_1m / retain-stringify | 1 | 18.73 / 16.75 / 60.80 / 34.05 |
| unicode_1m / retain-stringify | 32 | 48.69 / 46.70 / 89.12 / 61.77 |
| wide_1m / retain-parse | 1 | 24.20 / 24.09 / 65.52 / 36.30 |
| wide_1m / retain-parse | 16 | 71.38 / 71.27 / 112.45 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.28 / 30.23 / 68.59 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.55 / 45.50 / 86.41 / 53.45 |

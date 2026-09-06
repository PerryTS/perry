Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.251 / 0.250 / 0.027 / 0.019 | 1.00× | 13.55 / 13.56 / 57.53 / 35.67 |
| null / stringify | 2,000,000 | 0.066 / 0.007 / 0.027 / 0.029 | 9.58× | 34.89 / 13.66 / 59.53 / 129.75 |
| string_a / parse | 2,000,000 | 0.254 / 0.254 / 0.032 / 0.023 | 1.00× | 13.55 / 13.56 / 57.59 / 36.08 |
| string_a / stringify | 2,000,000 | 0.074 / 0.010 / 0.030 / 0.029 | 7.43× | 34.86 / 13.67 / 59.53 / 129.75 |
| empty_object / parse | 2,000,000 | 0.326 / 0.325 / 0.045 / 0.024 | 1.00× | 34.64 / 34.59 / 59.41 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.136 / 0.126 / 0.032 / 0.030 | 1.08× | 34.92 / 34.89 / 59.44 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.363 / 0.364 / 0.081 / 0.045 | 1.00× | 34.64 / 34.59 / 59.50 / 69.05 |
| tiny_object / stringify | 2,000,000 | 0.197 / 0.196 / 0.037 / 0.040 | 1.00× | 34.91 / 34.92 / 59.56 / 129.73 |
| small_record / parse | 583,137 | 0.749 / 0.749 / 0.332 / 0.252 | 1.00× | 35.02 / 34.97 / 59.61 / 79.86 |
| small_record / stringify | 1,245,459 | 0.550 / 0.567 / 0.109 / 0.121 | 0.97× | 35.98 / 35.95 / 59.61 / 249.78 |
| object_1k / parse | 554,656 | 0.590 / 0.589 / 0.544 / 0.238 | 1.00× | 35.05 / 35.02 / 61.69 / 71.14 |
| object_1k / stringify | 649,995 | 0.386 / 0.386 / 0.198 / 0.215 | 1.00× | 36.08 / 36.08 / 61.66 / 70.83 |
| records_array_16k / parse | 6,648 | 23.803 / 23.915 / 39.506 / 33.992 | 1.00× | 65.02 / 64.95 / 65.73 / 70.45 |
| records_array_16k / stringify | 10,041 | 35.870 / 33.099 / 13.207 / 23.205 | 1.08× | 36.28 / 36.19 / 61.83 / 70.98 |
| records_array_1m / parse | 92 | 1,542.011 / 1,550.848 / 2,685.109 / 2,143.120 | 0.99× | 62.31 / 62.22 / 92.70 / 79.08 |
| records_array_1m / stringify | 178 | 2,203.326 / 2,057.326 / 846.646 / 968.270 | 1.07× | 60.56 / 60.45 / 110.03 / 100.30 |
| records_object_1m / parse | 71 | 4,021.070 / 4,061.958 / 2,720.451 / 2,137.789 | 0.99× | 80.14 / 80.08 / 92.78 / 75.92 |
| records_object_1m / stringify | 175 | 2,201.303 / 2,076.543 / 843.891 / 968.377 | 1.06× | 60.55 / 60.47 / 107.48 / 100.30 |
| records_array_8m / parse | 10 | 13,794.900 / 13,870.100 / 33,499.200 / 20,696.000 | 0.99× | 102.75 / 102.66 / 243.97 / 131.88 |
| records_array_8m / stringify | 20 | 18,733.050 / 17,569.550 / 6,791.300 / 8,492.000 | 1.07× | 196.03 / 196.05 / 186.05 / 169.81 |
| records_object_8m / parse | 8 | 31,737.375 / 31,718.375 / 31,185.500 / 21,867.000 | 1.00× | 231.81 / 231.81 / 225.52 / 111.31 |
| records_object_8m / stringify | 20 | 18,794.000 / 17,518.150 / 6,875.550 / 8,446.100 | 1.07× | 196.05 / 196.08 / 186.00 / 169.72 |
| records_array_20m / parse | 3 | 78,636.000 / 78,767.000 / 95,139.667 / 58,692.667 | 1.00× | 315.83 / 315.81 / 325.95 / 187.95 |
| records_array_20m / stringify | 8 | 97,492.125 / 96,620.125 / 17,355.750 / 20,813.125 | 1.01× | 171.38 / 171.33 / 392.11 / 304.77 |
| records_object_20m / parse | 3 | 78,813.000 / 78,776.000 / 97,832.667 / 57,112.000 | 1.00× | 315.83 / 315.81 / 325.92 / 187.84 |
| records_object_20m / stringify | 8 | 97,658.750 / 96,206.750 / 17,379.500 / 21,027.875 | 1.02× | 171.36 / 171.34 / 392.12 / 304.89 |
| numbers_1m / parse | 95 | 1,560.400 / 1,600.200 / 3,120.979 / 3,203.484 | 0.98× | 64.55 / 64.52 / 90.19 / 68.17 |
| numbers_1m / stringify | 75 | 6,603.067 / 2,255.693 / 1,935.067 / 2,951.307 | 2.93× | 61.44 / 61.34 / 81.73 / 70.22 |
| long_string_1m / parse | 1,327 | 203.327 / 204.326 / 369.027 / 68.057 | 1.00× | 60.92 / 60.83 / 160.89 / 166.42 |
| long_string_1m / stringify | 1,034 | 203.978 / 204.883 / 106.540 / 96.673 | 1.00× | 64.02 / 63.98 / 153.81 / 123.59 |
| escaped_1m / parse | 82 | 2,040.793 / 2,039.256 / 1,736.585 / 2,083.951 | 1.00× | 58.30 / 58.20 / 74.02 / 64.20 |
| escaped_1m / stringify | 83 | 1,860.711 / 1,905.976 / 1,855.952 / 2,102.289 | 0.98× | 57.08 / 57.03 / 79.64 / 68.97 |
| unicode_1m / parse | 1,359 | 257.960 / 259.524 / 438.519 / 60.681 | 0.99× | 55.64 / 55.50 / 162.28 / 151.69 |
| unicode_1m / stringify | 669 | 256.490 / 255.106 / 420.725 / 451.825 | 1.01× | 58.34 / 58.28 / 138.48 / 97.27 |
| wide_1m / parse | 36 | 21,035.583 / 21,036.167 / 4,935.389 / 4,163.694 | 1.00× | 176.77 / 176.77 / 110.53 / 86.12 |
| wide_1m / stringify | 215 | 2,485.079 / 2,470.177 / 6,365.809 / 664.893 | 1.01× | 71.00 / 70.97 / 116.44 / 84.58 |
| heterogeneous_1m / parse | 78 | 1,866.141 / 1,875.808 / 3,900.000 / 2,956.641 | 0.99× | 68.19 / 68.11 / 92.19 / 80.72 |
| heterogeneous_1m / stringify | 168 | 4,483.690 / 4,519.774 / 903.494 / 1,063.054 | 0.99× | 59.27 / 59.20 / 110.67 / 100.48 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.33 / 12.30 / 52.62 / 28.23 |
| tiny_object / retain-parse | 200,000 | 24.91 / 24.86 / 74.80 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.58 / 52.70 / 28.27 |
| tiny_object / retain-stringify | 200,000 | 36.41 / 36.38 / 71.25 / 47.03 |
| small_record / retain-parse | 1 | 12.36 / 12.30 / 52.66 / 28.27 |
| small_record / retain-parse | 100,000 | 57.00 / 56.95 / 85.50 / 52.66 |
| small_record / retain-stringify | 1 | 12.61 / 12.62 / 52.77 / 28.31 |
| small_record / retain-stringify | 100,000 | 31.44 / 31.45 / 80.30 / 51.05 |
| records_array_1m / retain-parse | 1 | 17.97 / 17.94 / 58.31 / 31.66 |
| records_array_1m / retain-parse | 16 | 41.38 / 41.34 / 88.77 / 55.92 |
| records_array_1m / retain-stringify | 1 | 20.17 / 20.11 / 60.53 / 34.41 |
| records_array_1m / retain-stringify | 16 | 34.06 / 33.98 / 76.31 / 47.20 |
| records_object_1m / retain-parse | 1 | 15.98 / 15.94 / 58.41 / 31.61 |
| records_object_1m / retain-parse | 16 | 67.14 / 67.03 / 88.72 / 55.91 |
| records_object_1m / retain-stringify | 1 | 20.16 / 20.17 / 60.48 / 34.41 |
| records_object_1m / retain-stringify | 16 | 34.06 / 34.06 / 76.41 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.62 / 48.58 / 94.23 / 52.28 |
| records_array_8m / retain-parse | 4 | 89.30 / 89.16 / 146.92 / 91.80 |
| records_array_8m / retain-stringify | 1 | 75.77 / 75.67 / 114.19 / 73.44 |
| records_array_8m / retain-stringify | 4 | 95.06 / 94.97 / 143.89 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.94 / 42.95 / 94.31 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.44 / 113.41 / 146.97 / 92.03 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.70 / 114.14 / 73.44 |
| records_object_8m / retain-stringify | 4 | 95.06 / 95.00 / 144.02 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.44 / 15.39 / 56.48 / 30.58 |
| long_string_1m / retain-parse | 32 | 56.98 / 56.84 / 89.11 / 61.66 |
| long_string_1m / retain-stringify | 1 | 21.16 / 21.17 / 60.52 / 32.61 |
| long_string_1m / retain-stringify | 32 | 59.89 / 59.83 / 92.34 / 63.72 |
| unicode_1m / retain-parse | 1 | 14.62 / 14.56 / 57.70 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.84 / 44.78 / 86.25 / 58.91 |
| unicode_1m / retain-stringify | 1 | 18.73 / 18.72 / 60.81 / 34.03 |
| unicode_1m / retain-stringify | 32 | 48.69 / 48.69 / 89.16 / 61.75 |
| wide_1m / retain-parse | 1 | 24.19 / 24.12 / 65.59 / 36.28 |
| wide_1m / retain-parse | 16 | 71.38 / 71.31 / 112.47 / 68.91 |
| wide_1m / retain-stringify | 1 | 30.28 / 30.31 / 68.58 / 39.20 |
| wide_1m / retain-stringify | 16 | 45.55 / 45.58 / 86.45 / 53.44 |

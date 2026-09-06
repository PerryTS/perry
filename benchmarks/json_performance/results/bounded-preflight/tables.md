Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.013 / 0.027 / 0.019 | 1.02× | 13.47 / 13.38 / 57.55 / 35.69 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.58 / 13.48 / 59.53 / 129.77 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.02× | 13.47 / 13.38 / 57.59 / 36.08 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.56 / 13.50 / 59.53 / 129.83 |
| empty_object / parse | 2,000,000 | 0.328 / 0.320 / 0.045 / 0.024 | 1.02× | 34.52 / 34.48 / 59.42 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.01× | 13.69 / 13.61 / 59.48 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.374 / 0.356 / 0.082 / 0.045 | 1.05× | 34.52 / 34.50 / 59.42 / 69.03 |
| tiny_object / stringify | 2,000,000 | 0.140 / 0.140 / 0.037 / 0.039 | 1.00× | 34.69 / 34.70 / 59.53 / 129.77 |
| small_record / parse | 581,865 | 0.752 / 0.676 / 0.344 / 0.252 | 1.11× | 34.91 / 34.89 / 59.39 / 79.84 |
| small_record / stringify | 1,267,159 | 0.526 / 0.527 / 0.108 / 0.120 | 1.00× | 35.80 / 35.78 / 59.61 / 253.47 |
| object_1k / parse | 547,820 | 0.592 / 0.588 / 0.546 / 0.237 | 1.01× | 34.94 / 34.92 / 61.66 / 71.11 |
| object_1k / stringify | 645,855 | 0.287 / 0.287 / 0.199 / 0.216 | 1.00× | 35.83 / 35.84 / 61.59 / 70.80 |
| records_array_16k / parse | 6,729 | 23.680 / 23.623 / 42.321 / 33.941 | 1.00× | 64.94 / 64.88 / 65.70 / 70.42 |
| records_array_16k / stringify | 10,014 | 24.267 / 24.155 / 13.174 / 23.243 | 1.00× | 36.11 / 36.09 / 61.73 / 71.02 |
| records_array_1m / parse | 94 | 1,506.340 / 1,506.606 / 2,870.340 / 2,131.564 | 1.00× | 62.23 / 62.12 / 92.98 / 79.11 |
| records_array_1m / stringify | 177 | 1,547.186 / 1,560.785 / 842.492 / 969.102 | 0.99× | 60.45 / 60.38 / 109.16 / 100.30 |
| records_object_1m / parse | 71 | 3,963.859 / 4,010.028 / 2,764.915 / 2,136.746 | 0.99× | 80.03 / 80.00 / 92.77 / 75.94 |
| records_object_1m / stringify | 176 | 1,559.455 / 1,561.278 / 848.062 / 970.409 | 1.00× | 60.45 / 60.34 / 108.42 / 100.25 |
| records_array_8m / parse | 10 | 13,564.300 / 13,560.500 / 34,483.500 / 20,961.500 | 1.00× | 102.67 / 102.59 / 244.14 / 131.02 |
| records_array_8m / stringify | 20 | 13,487.250 / 13,513.650 / 6,806.700 / 8,468.100 | 1.00× | 195.95 / 195.89 / 185.97 / 169.84 |
| records_object_8m / parse | 8 | 30,956.750 / 31,481.750 / 29,251.500 / 21,884.000 | 0.98× | 231.77 / 231.69 / 225.44 / 111.53 |
| records_object_8m / stringify | 20 | 13,453.450 / 13,549.400 / 6,827.400 / 8,430.850 | 0.99× | 195.97 / 195.88 / 185.97 / 169.73 |
| records_array_20m / parse | 3 | 76,580.667 / 78,659.667 / 95,303.000 / 56,612.000 | 0.97× | 315.80 / 315.69 / 325.92 / 188.09 |
| records_array_20m / stringify | 8 | 85,939.125 / 86,439.250 / 17,339.500 / 20,838.250 | 0.99× | 171.30 / 171.17 / 392.11 / 304.92 |
| records_object_20m / parse | 3 | 76,619.000 / 77,986.333 / 94,218.667 / 56,610.667 | 0.98× | 315.80 / 315.69 / 326.02 / 187.81 |
| records_object_20m / stringify | 8 | 86,177.875 / 86,389.125 / 17,370.125 / 20,835.500 | 1.00× | 171.28 / 171.16 / 392.11 / 304.77 |
| numbers_1m / parse | 89 | 1,696.506 / 1,694.584 / 3,128.966 / 3,190.831 | 1.00× | 64.48 / 64.41 / 84.53 / 65.14 |
| numbers_1m / stringify | 96 | 1,603.135 / 1,611.667 / 1,979.198 / 2,954.469 | 0.99× | 61.38 / 61.31 / 92.27 / 72.22 |
| long_string_1m / parse | 1,180 | 203.224 / 204.909 / 369.162 / 68.371 | 0.99× | 60.81 / 60.75 / 155.88 / 162.38 |
| long_string_1m / stringify | 1,169 | 174.155 / 178.109 / 105.295 / 97.320 | 0.98× | 62.00 / 61.95 / 159.05 / 153.58 |
| escaped_1m / parse | 82 | 2,040.610 / 2,039.232 / 1,733.280 / 2,083.866 | 1.00× | 58.27 / 58.16 / 74.03 / 64.16 |
| escaped_1m / stringify | 83 | 1,863.867 / 1,863.747 / 1,852.373 / 2,097.916 | 1.00× | 57.05 / 56.92 / 79.66 / 68.95 |
| unicode_1m / parse | 1,488 | 258.346 / 257.501 / 438.011 / 60.061 | 1.00× | 56.55 / 56.50 / 166.08 / 151.75 |
| unicode_1m / stringify | 1,443 | 139.197 / 137.144 / 412.702 / 451.106 | 1.01× | 57.19 / 57.17 / 158.64 / 162.45 |
| wide_1m / parse | 36 | 21,050.778 / 20,878.528 / 4,958.194 / 4,164.528 | 1.01× | 176.72 / 176.64 / 110.50 / 86.16 |
| wide_1m / stringify | 214 | 1,826.846 / 1,695.542 / 6,368.023 / 665.350 | 1.08× | 70.94 / 70.84 / 116.59 / 84.69 |
| heterogeneous_1m / parse | 78 | 1,889.769 / 1,889.205 / 3,972.821 / 2,993.795 | 1.00× | 68.11 / 68.05 / 92.03 / 80.84 |
| heterogeneous_1m / stringify | 167 | 4,522.982 / 4,593.335 / 906.725 / 1,057.970 | 0.98× | 59.19 / 59.09 / 110.58 / 101.34 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.19 / 12.22 / 52.66 / 28.25 |
| tiny_object / retain-parse | 200,000 | 24.73 / 24.77 / 74.69 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.33 / 12.33 / 52.78 / 28.28 |
| tiny_object / retain-stringify | 200,000 | 24.86 / 24.91 / 71.23 / 47.02 |
| small_record / retain-parse | 1 | 12.22 / 12.22 / 52.67 / 28.34 |
| small_record / retain-parse | 100,000 | 56.80 / 56.81 / 85.67 / 52.69 |
| small_record / retain-stringify | 1 | 12.47 / 12.47 / 52.78 / 28.34 |
| small_record / retain-stringify | 100,000 | 31.27 / 31.28 / 80.28 / 51.03 |
| records_array_1m / retain-parse | 1 | 17.83 / 17.86 / 58.41 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.23 / 41.25 / 88.62 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.05 / 20.00 / 60.62 / 34.41 |
| records_array_1m / retain-stringify | 16 | 33.92 / 33.89 / 76.34 / 47.20 |
| records_object_1m / retain-parse | 1 | 15.86 / 15.91 / 58.41 / 31.62 |
| records_object_1m / retain-parse | 16 | 66.91 / 66.95 / 88.78 / 55.91 |
| records_object_1m / retain-stringify | 1 | 20.02 / 20.03 / 60.50 / 34.42 |
| records_object_1m / retain-stringify | 16 | 33.91 / 33.92 / 76.28 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.48 / 48.47 / 94.28 / 52.30 |
| records_array_8m / retain-parse | 4 | 89.20 / 89.12 / 146.95 / 91.81 |
| records_array_8m / retain-stringify | 1 | 75.50 / 75.53 / 114.20 / 73.44 |
| records_array_8m / retain-stringify | 4 | 94.80 / 94.83 / 143.94 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.83 / 42.91 / 94.33 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.25 / 113.30 / 146.89 / 91.77 |
| records_object_8m / retain-stringify | 1 | 75.50 / 75.53 / 114.14 / 73.44 |
| records_object_8m / retain-stringify | 4 | 94.80 / 94.83 / 143.94 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.25 / 15.34 / 56.52 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.81 / 56.81 / 89.05 / 61.61 |
| long_string_1m / retain-stringify | 1 | 18.72 / 18.78 / 60.52 / 32.67 |
| long_string_1m / retain-stringify | 32 | 58.77 / 58.75 / 92.23 / 63.70 |
| unicode_1m / retain-parse | 1 | 14.47 / 14.53 / 57.62 / 31.22 |
| unicode_1m / retain-parse | 32 | 44.66 / 44.73 / 86.27 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.45 / 16.52 / 60.77 / 34.06 |
| unicode_1m / retain-stringify | 32 | 46.41 / 46.48 / 89.17 / 62.17 |
| wide_1m / retain-parse | 1 | 24.06 / 24.09 / 65.67 / 36.27 |
| wide_1m / retain-parse | 16 | 71.25 / 71.27 / 112.52 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.14 / 30.16 / 68.56 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.41 / 45.42 / 86.39 / 53.45 |

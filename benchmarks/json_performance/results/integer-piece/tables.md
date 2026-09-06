Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 0.98× | 13.52 / 13.47 / 57.58 / 35.75 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.58 / 13.56 / 59.47 / 129.77 |
| string_a / parse | 2,000,000 | 0.016 / 0.017 / 0.032 / 0.023 | 0.98× | 13.52 / 13.47 / 57.55 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.03× | 13.58 / 13.55 / 59.56 / 129.80 |
| empty_object / parse | 2,000,000 | 0.315 / 0.325 / 0.045 / 0.024 | 0.97× | 34.41 / 34.45 / 59.44 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.00× | 13.72 / 13.67 / 59.55 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.350 / 0.360 / 0.083 / 0.045 | 0.97× | 34.41 / 34.47 / 59.39 / 69.05 |
| tiny_object / stringify | 2,000,000 | 0.137 / 0.095 / 0.037 / 0.039 | 1.44× | 34.62 / 34.67 / 59.53 / 129.75 |
| small_record / parse | 582,382 | 0.670 / 0.682 / 0.344 / 0.253 | 0.98× | 34.80 / 34.84 / 59.59 / 79.81 |
| small_record / stringify | 1,279,317 | 0.403 / 0.316 / 0.109 / 0.120 | 1.27× | 35.75 / 35.81 / 59.61 / 255.56 |
| object_1k / parse | 563,159 | 0.585 / 0.611 / 0.540 / 0.237 | 0.96× | 34.83 / 34.86 / 61.70 / 71.11 |
| object_1k / stringify | 640,798 | 0.286 / 0.245 / 0.199 / 0.215 | 1.17× | 35.77 / 35.80 / 61.72 / 70.80 |
| records_array_16k / parse | 6,635 | 24.074 / 23.848 / 39.991 / 33.955 | 1.01× | 64.78 / 64.92 / 65.62 / 70.39 |
| records_array_16k / stringify | 10,013 | 24.516 / 24.136 / 13.169 / 23.150 | 1.02× | 36.06 / 36.08 / 61.73 / 71.00 |
| records_array_1m / parse | 93 | 1,534.699 / 1,520.720 / 2,770.634 / 2,139.935 | 1.01× | 62.08 / 62.19 / 92.77 / 79.12 |
| records_array_1m / stringify | 176 | 1,562.017 / 1,551.790 / 846.403 / 972.119 | 1.01× | 60.31 / 60.41 / 108.33 / 100.31 |
| records_object_1m / parse | 71 | 4,015.676 / 4,029.606 / 2,888.282 / 2,138.507 | 1.00× | 79.94 / 79.97 / 92.67 / 75.89 |
| records_object_1m / stringify | 176 | 1,567.153 / 1,554.034 / 842.892 / 972.057 | 1.01× | 60.30 / 60.39 / 108.41 / 100.27 |
| records_array_8m / parse | 10 | 13,751.500 / 13,656.400 / 33,381.800 / 20,870.200 | 1.01× | 102.52 / 102.62 / 243.95 / 131.19 |
| records_array_8m / stringify | 20 | 13,561.450 / 13,525.050 / 6,813.650 / 8,458.250 | 1.00× | 195.84 / 195.91 / 186.03 / 169.88 |
| records_object_8m / parse | 8 | 31,509.375 / 31,740.750 / 29,831.750 / 21,690.125 | 0.99× | 231.64 / 231.67 / 225.64 / 110.72 |
| records_object_8m / stringify | 20 | 13,572.800 / 13,504.250 / 6,813.600 / 8,484.550 | 1.01× | 195.86 / 195.91 / 186.00 / 189.94 |
| records_array_20m / parse | 3 | 77,877.333 / 78,415.333 / 95,018.667 / 57,027.333 | 0.99× | 315.66 / 315.66 / 326.19 / 187.69 |
| records_array_20m / stringify | 8 | 86,127.125 / 86,028.625 / 17,513.750 / 20,864.375 | 1.00× | 171.19 / 171.19 / 392.11 / 304.86 |
| records_object_20m / parse | 3 | 77,816.667 / 78,177.667 / 94,575.667 / 56,541.333 | 1.00× | 315.66 / 315.67 / 326.09 / 188.05 |
| records_object_20m / stringify | 8 | 86,021.125 / 86,052.750 / 17,360.375 / 20,841.125 | 1.00× | 171.17 / 171.19 / 392.20 / 304.80 |
| numbers_1m / parse | 87 | 1,705.195 / 1,729.230 / 3,129.552 / 3,194.552 | 0.99× | 64.33 / 64.47 / 82.44 / 65.17 |
| numbers_1m / stringify | 94 | 1,641.830 / 1,648.564 / 1,983.117 / 2,953.798 | 1.00× | 61.23 / 61.84 / 90.30 / 72.22 |
| long_string_1m / parse | 1,279 | 205.123 / 216.952 / 368.513 / 67.490 | 0.95× | 60.64 / 60.78 / 158.92 / 153.39 |
| long_string_1m / stringify | 1,140 | 174.304 / 174.863 / 105.646 / 97.725 | 1.00× | 61.84 / 61.94 / 157.92 / 159.58 |
| escaped_1m / parse | 82 | 2,041.427 / 2,039.329 / 1,745.549 / 2,088.012 | 1.00× | 58.03 / 58.17 / 74.00 / 64.19 |
| escaped_1m / stringify | 83 | 1,907.747 / 1,862.855 / 1,850.349 / 2,096.446 | 1.02× | 56.88 / 56.95 / 79.59 / 68.94 |
| unicode_1m / parse | 1,477 | 258.206 / 265.053 / 438.233 / 60.491 | 0.97× | 55.36 / 55.50 / 165.06 / 156.14 |
| unicode_1m / stringify | 1,396 | 137.044 / 137.846 / 411.547 / 449.511 | 0.99× | 57.06 / 57.17 / 157.58 / 131.17 |
| wide_1m / parse | 36 | 20,883.500 / 20,878.500 / 4,956.917 / 4,153.278 | 1.00× | 176.53 / 176.64 / 110.48 / 86.12 |
| wide_1m / stringify | 215 | 1,707.423 / 1,708.228 / 6,367.526 / 665.851 | 1.00× | 70.78 / 70.89 / 116.42 / 84.36 |
| heterogeneous_1m / parse | 77 | 1,955.052 / 1,913.571 / 3,879.636 / 2,959.545 | 1.02× | 67.95 / 68.08 / 92.59 / 80.59 |
| heterogeneous_1m / stringify | 168 | 4,379.988 / 4,400.631 / 907.738 / 1,064.690 | 1.00× | 59.06 / 59.12 / 110.55 / 101.36 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.23 / 12.17 / 52.70 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.80 / 24.77 / 74.67 / 50.17 |
| tiny_object / retain-stringify | 1 | 12.36 / 12.28 / 52.72 / 28.31 |
| tiny_object / retain-stringify | 200,000 | 24.91 / 24.84 / 71.20 / 47.02 |
| small_record / retain-parse | 1 | 12.27 / 12.19 / 52.64 / 28.27 |
| small_record / retain-parse | 100,000 | 56.73 / 56.78 / 85.72 / 52.66 |
| small_record / retain-stringify | 1 | 12.42 / 12.33 / 52.88 / 28.31 |
| small_record / retain-stringify | 100,000 | 28.11 / 28.05 / 80.31 / 51.05 |
| records_array_1m / retain-parse | 1 | 17.89 / 17.83 / 58.41 / 31.69 |
| records_array_1m / retain-parse | 16 | 41.28 / 41.23 / 88.70 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.12 / 19.95 / 60.62 / 34.42 |
| records_array_1m / retain-stringify | 16 | 34.00 / 33.86 / 76.28 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.91 / 15.84 / 58.44 / 31.62 |
| records_object_1m / retain-parse | 16 | 66.88 / 66.89 / 88.75 / 55.88 |
| records_object_1m / retain-stringify | 1 | 20.11 / 19.97 / 60.53 / 34.42 |
| records_object_1m / retain-stringify | 16 | 33.98 / 33.88 / 76.33 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.55 / 48.47 / 94.30 / 52.30 |
| records_array_8m / retain-parse | 4 | 89.06 / 89.19 / 146.86 / 91.83 |
| records_array_8m / retain-stringify | 1 | 75.53 / 75.52 / 114.16 / 73.45 |
| records_array_8m / retain-stringify | 4 | 94.83 / 94.81 / 143.98 / 94.47 |
| records_object_8m / retain-parse | 1 | 42.86 / 42.86 / 94.33 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.22 / 113.25 / 146.92 / 91.83 |
| records_object_8m / retain-stringify | 1 | 75.53 / 75.53 / 114.14 / 73.45 |
| records_object_8m / retain-stringify | 4 | 94.83 / 94.83 / 143.94 / 94.44 |
| long_string_1m / retain-parse | 1 | 15.31 / 15.31 / 56.47 / 30.52 |
| long_string_1m / retain-parse | 32 | 56.66 / 56.86 / 89.12 / 61.59 |
| long_string_1m / retain-stringify | 1 | 18.77 / 18.73 / 60.53 / 32.59 |
| long_string_1m / retain-stringify | 32 | 58.62 / 58.77 / 92.28 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.52 / 14.48 / 57.69 / 31.30 |
| unicode_1m / retain-parse | 32 | 44.72 / 44.70 / 86.25 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.75 / 16.69 / 60.83 / 34.02 |
| unicode_1m / retain-stringify | 32 | 46.70 / 46.66 / 89.17 / 61.75 |
| wide_1m / retain-parse | 1 | 24.08 / 24.03 / 65.61 / 36.34 |
| wide_1m / retain-parse | 16 | 71.27 / 71.22 / 112.45 / 68.84 |
| wide_1m / retain-stringify | 1 | 30.22 / 30.12 / 68.56 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.48 / 45.50 / 86.44 / 53.47 |

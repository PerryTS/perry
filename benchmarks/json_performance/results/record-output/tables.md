Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.56 / 13.52 / 57.59 / 35.72 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.61 / 13.59 / 59.50 / 129.72 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.56 / 13.52 / 57.56 / 36.06 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 0.97× | 13.61 / 13.58 / 59.53 / 129.73 |
| empty_object / parse | 2,000,000 | 0.321 / 0.314 / 0.045 / 0.024 | 1.02× | 34.58 / 34.41 / 59.36 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 0.99× | 13.72 / 13.72 / 59.61 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.353 / 0.350 / 0.081 / 0.045 | 1.01× | 34.58 / 34.41 / 59.50 / 69.03 |
| tiny_object / stringify | 2,000,000 | 0.140 / 0.136 / 0.037 / 0.040 | 1.03× | 34.78 / 34.61 / 59.62 / 129.75 |
| small_record / parse | 585,890 | 0.671 / 0.669 / 0.342 / 0.251 | 1.00× | 34.97 / 34.80 / 59.58 / 79.83 |
| small_record / stringify | 1,293,800 | 0.524 / 0.402 / 0.108 / 0.121 | 1.30× | 35.95 / 35.75 / 59.72 / 258.00 |
| object_1k / parse | 541,189 | 0.589 / 0.586 / 0.542 / 0.238 | 1.00× | 35.02 / 34.83 / 61.67 / 71.11 |
| object_1k / stringify | 640,228 | 0.288 / 0.287 / 0.201 / 0.215 | 1.00× | 35.92 / 35.77 / 61.73 / 70.81 |
| records_array_16k / parse | 6,729 | 23.634 / 24.031 / 38.600 / 33.970 | 0.98× | 65.02 / 64.78 / 65.80 / 70.42 |
| records_array_16k / stringify | 9,944 | 24.436 / 24.252 / 13.172 / 23.261 | 1.01× | 36.23 / 36.06 / 61.81 / 71.00 |
| records_array_1m / parse | 94 | 1,511.213 / 1,530.809 / 2,752.202 / 2,134.819 | 0.99× | 66.89 / 62.05 / 92.70 / 79.08 |
| records_array_1m / stringify | 176 | 1,557.955 / 1,558.739 / 846.932 / 970.551 | 1.00× | 60.52 / 60.33 / 108.22 / 100.27 |
| records_object_1m / parse | 71 | 3,997.662 / 4,019.437 / 2,740.915 / 2,138.662 | 0.99× | 80.11 / 79.92 / 92.73 / 75.92 |
| records_object_1m / stringify | 175 | 1,566.120 / 1,566.703 / 847.280 / 971.446 | 1.00× | 60.50 / 60.30 / 107.53 / 100.27 |
| records_array_8m / parse | 10 | 13,471.100 / 13,748.900 / 34,636.900 / 20,831.800 | 0.98× | 88.17 / 102.50 / 244.00 / 131.23 |
| records_array_8m / stringify | 20 | 13,541.450 / 13,549.750 / 6,783.600 / 8,454.200 | 1.00× | 196.03 / 195.88 / 186.02 / 169.62 |
| records_object_8m / parse | 8 | 31,427.875 / 31,491.625 / 30,114.500 / 21,716.750 | 1.00× | 231.80 / 231.62 / 225.28 / 111.64 |
| records_object_8m / stringify | 20 | 13,550.200 / 13,581.200 / 6,833.300 / 8,489.800 | 1.00× | 196.03 / 195.86 / 185.97 / 169.67 |
| records_array_20m / parse | 3 | 77,644.000 / 77,826.667 / 97,782.000 / 56,832.667 | 1.00× | 315.81 / 315.61 / 325.95 / 187.81 |
| records_array_20m / stringify | 8 | 86,018.125 / 86,501.875 / 17,354.000 / 21,003.875 | 0.99× | 171.36 / 171.14 / 392.11 / 304.81 |
| records_object_20m / parse | 3 | 77,529.667 / 77,804.667 / 94,889.333 / 56,547.000 | 1.00× | 315.81 / 315.61 / 325.91 / 188.27 |
| records_object_20m / stringify | 8 | 85,973.125 / 86,022.875 / 17,364.500 / 20,778.875 | 1.00× | 171.34 / 171.12 / 392.14 / 304.89 |
| numbers_1m / parse | 88 | 1,725.307 / 1,727.750 / 3,129.739 / 3,200.250 | 1.00× | 65.22 / 64.33 / 83.41 / 65.17 |
| numbers_1m / stringify | 95 | 1,605.589 / 1,644.474 / 1,981.768 / 2,951.063 | 0.98× | 61.41 / 61.77 / 91.30 / 72.20 |
| long_string_1m / parse | 1,261 | 204.906 / 204.825 / 368.796 / 67.643 | 1.00× | 60.88 / 60.64 / 157.80 / 156.36 |
| long_string_1m / stringify | 1,161 | 174.324 / 173.995 / 105.844 / 97.479 | 1.00× | 62.03 / 61.83 / 157.92 / 157.59 |
| escaped_1m / parse | 83 | 2,040.759 / 2,038.518 / 1,733.120 / 2,084.217 | 1.00× | 58.27 / 58.03 / 73.89 / 64.88 |
| escaped_1m / stringify | 81 | 1,907.494 / 1,906.568 / 1,853.086 / 2,094.000 | 1.00× | 57.05 / 56.89 / 79.62 / 68.94 |
| unicode_1m / parse | 1,494 | 258.635 / 257.178 / 437.642 / 60.096 | 1.01× | 55.59 / 55.36 / 165.78 / 151.75 |
| unicode_1m / stringify | 1,456 | 137.579 / 138.532 / 413.397 / 450.198 | 0.99× | 57.27 / 57.06 / 159.62 / 130.25 |
| wide_1m / parse | 36 | 20,945.000 / 20,963.083 / 4,964.500 / 4,154.750 | 1.00× | 176.77 / 176.53 / 110.55 / 86.14 |
| wide_1m / stringify | 216 | 1,689.361 / 1,706.921 / 6,355.875 / 665.704 | 0.99× | 70.95 / 70.78 / 116.39 / 83.17 |
| heterogeneous_1m / parse | 77 | 1,895.753 / 1,960.714 / 3,855.377 / 2,984.675 | 0.97× | 71.81 / 67.94 / 92.55 / 80.80 |
| heterogeneous_1m / stringify | 168 | 4,383.190 / 4,379.869 / 903.190 / 1,064.940 | 1.00× | 59.25 / 59.06 / 110.59 / 100.50 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.25 / 12.23 / 52.64 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.83 / 24.80 / 74.75 / 50.17 |
| tiny_object / retain-stringify | 1 | 12.36 / 12.36 / 52.77 / 28.27 |
| tiny_object / retain-stringify | 200,000 | 24.92 / 24.91 / 71.25 / 47.02 |
| small_record / retain-parse | 1 | 12.28 / 12.27 / 52.66 / 28.30 |
| small_record / retain-parse | 100,000 | 56.86 / 56.75 / 85.41 / 52.64 |
| small_record / retain-stringify | 1 | 12.53 / 12.41 / 52.72 / 28.31 |
| small_record / retain-stringify | 100,000 | 31.36 / 28.11 / 80.31 / 51.05 |
| records_array_1m / retain-parse | 1 | 17.97 / 17.89 / 58.47 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.39 / 41.30 / 88.69 / 55.89 |
| records_array_1m / retain-stringify | 1 | 20.14 / 20.09 / 60.48 / 34.41 |
| records_array_1m / retain-stringify | 16 | 34.00 / 33.98 / 76.31 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.94 / 15.91 / 58.38 / 31.62 |
| records_object_1m / retain-parse | 16 | 67.03 / 66.89 / 88.61 / 55.88 |
| records_object_1m / retain-stringify | 1 | 20.11 / 20.11 / 60.53 / 34.42 |
| records_object_1m / retain-stringify | 16 | 33.98 / 33.98 / 76.39 / 47.20 |
| records_array_8m / retain-parse | 1 | 48.64 / 48.52 / 94.30 / 52.31 |
| records_array_8m / retain-parse | 4 | 87.03 / 89.03 / 146.95 / 92.03 |
| records_array_8m / retain-stringify | 1 | 75.64 / 75.53 / 114.12 / 73.45 |
| records_array_8m / retain-stringify | 4 | 94.94 / 94.83 / 144.02 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.89 / 42.91 / 94.27 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.33 / 113.23 / 146.81 / 91.81 |
| records_object_8m / retain-stringify | 1 | 75.64 / 75.53 / 114.20 / 73.45 |
| records_object_8m / retain-stringify | 4 | 94.94 / 94.83 / 143.98 / 94.45 |
| long_string_1m / retain-parse | 1 | 15.34 / 15.36 / 56.50 / 30.52 |
| long_string_1m / retain-parse | 32 | 56.92 / 56.69 / 89.16 / 61.62 |
| long_string_1m / retain-stringify | 1 | 18.78 / 18.78 / 60.50 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.86 / 58.62 / 92.28 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.55 / 14.52 / 57.67 / 31.27 |
| unicode_1m / retain-parse | 32 | 44.75 / 44.72 / 86.27 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.78 / 16.75 / 60.81 / 34.08 |
| unicode_1m / retain-stringify | 32 | 46.72 / 46.70 / 89.11 / 61.72 |
| wide_1m / retain-parse | 1 | 24.16 / 24.09 / 65.59 / 36.30 |
| wide_1m / retain-parse | 16 | 71.33 / 71.27 / 112.44 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.16 / 30.22 / 68.59 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.44 / 45.48 / 86.39 / 53.44 |

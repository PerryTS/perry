Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.47 / 13.58 / 57.56 / 35.72 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.56 / 13.67 / 59.55 / 129.77 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.47 / 13.58 / 57.62 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.56 / 13.67 / 59.48 / 129.81 |
| empty_object / parse | 2,000,000 | 0.058 / 0.058 / 0.045 / 0.024 | 1.01× | 34.72 / 34.88 / 59.45 / 69.00 |
| empty_object / stringify | 2,000,000 | 0.047 / 0.047 / 0.032 / 0.030 | 1.00× | 13.66 / 13.78 / 59.58 / 129.78 |
| tiny_object / parse | 2,000,000 | 0.120 / 0.121 / 0.080 / 0.045 | 0.99× | 34.73 / 34.88 / 59.50 / 69.06 |
| tiny_object / stringify | 2,000,000 | 0.091 / 0.091 / 0.037 / 0.039 | 1.01× | 34.72 / 34.88 / 59.55 / 129.73 |
| small_record / parse | 588,332 | 0.670 / 0.670 / 0.331 / 0.253 | 1.00× | 34.47 / 34.64 / 59.55 / 79.86 |
| small_record / stringify | 1,289,629 | 0.282 / 0.281 / 0.108 / 0.120 | 1.00× | 35.86 / 36.03 / 59.67 / 257.30 |
| object_1k / parse | 544,670 | 0.595 / 0.594 / 0.545 / 0.237 | 1.00× | 34.95 / 35.12 / 61.59 / 71.16 |
| object_1k / stringify | 656,935 | 0.234 / 0.233 / 0.198 / 0.215 | 1.01× | 35.86 / 36.02 / 61.75 / 70.83 |
| records_array_16k / parse | 6,710 | 23.662 / 23.797 / 41.938 / 34.068 | 0.99× | 64.89 / 65.02 / 65.80 / 70.42 |
| records_array_16k / stringify | 10,000 | 23.429 / 23.368 / 13.166 / 23.277 | 1.00× | 36.16 / 36.28 / 61.75 / 71.03 |
| records_array_1m / parse | 96 | 1,542.969 / 1,553.250 / 2,773.323 / 2,134.948 | 0.99× | 62.11 / 62.20 / 93.42 / 79.14 |
| records_array_1m / stringify | 176 | 1,497.562 / 1,497.523 / 847.528 / 969.108 | 1.00× | 59.34 / 59.44 / 108.23 / 101.58 |
| records_object_1m / parse | 71 | 3,895.324 / 3,926.690 / 2,733.577 / 2,137.239 | 0.99× | 60.17 / 60.30 / 92.70 / 75.94 |
| records_object_1m / stringify | 175 | 1,500.051 / 1,502.069 / 842.469 / 971.349 | 1.00× | 59.33 / 59.44 / 107.31 / 100.30 |
| records_array_8m / parse | 10 | 13,334.900 / 13,331.800 / 34,322.700 / 20,870.800 | 1.00× | 99.50 / 99.61 / 243.91 / 131.31 |
| records_array_8m / stringify | 21 | 12,904.524 / 12,952.095 / 6,794.524 / 8,459.143 | 1.00× | 193.66 / 193.83 / 186.06 / 176.64 |
| records_object_8m / parse | 8 | 30,187.000 / 30,529.000 / 30,059.625 / 21,734.125 | 0.99× | 166.72 / 166.84 / 225.39 / 111.33 |
| records_object_8m / stringify | 20 | 12,968.850 / 13,041.450 / 6,805.850 / 8,424.050 | 0.99× | 193.66 / 193.84 / 185.98 / 189.72 |
| records_array_20m / parse | 3 | 74,537.000 / 75,642.333 / 94,071.667 / 56,775.667 | 0.99× | 223.83 / 223.95 / 326.03 / 188.03 |
| records_array_20m / stringify | 8 | 81,256.250 / 81,800.125 / 17,356.500 / 20,763.375 | 0.99× | 158.77 / 158.80 / 392.14 / 304.89 |
| records_object_20m / parse | 3 | 74,435.333 / 75,327.000 / 95,569.333 / 56,702.667 | 0.99× | 223.81 / 223.95 / 325.91 / 187.98 |
| records_object_20m / stringify | 8 | 81,417.625 / 81,707.625 / 17,334.125 / 20,878.875 | 1.00× | 158.75 / 158.81 / 392.14 / 304.89 |
| numbers_1m / parse | 91 | 1,712.626 / 1,716.549 / 3,126.407 / 3,203.154 | 1.00× | 64.92 / 65.05 / 86.58 / 67.19 |
| numbers_1m / stringify | 96 | 1,642.083 / 1,607.021 / 1,982.385 / 2,952.479 | 1.02× | 61.81 / 61.39 / 92.36 / 72.25 |
| long_string_1m / parse | 1,295 | 203.876 / 204.122 / 368.936 / 66.153 | 1.00× | 60.77 / 60.91 / 159.92 / 124.42 |
| long_string_1m / stringify | 1,196 | 173.329 / 173.972 / 105.006 / 97.663 | 1.00× | 61.91 / 62.05 / 158.94 / 165.59 |
| escaped_1m / parse | 82 | 2,040.390 / 2,044.268 / 1,732.927 / 2,086.488 | 1.00× | 58.14 / 58.28 / 73.86 / 64.17 |
| escaped_1m / stringify | 169 | 950.237 / 951.627 / 1,895.917 / 2,084.947 | 1.00× | 57.12 / 57.27 / 124.67 / 74.75 |
| unicode_1m / parse | 1,440 | 256.508 / 257.038 / 438.131 / 60.552 | 1.00× | 55.50 / 55.58 / 164.08 / 151.77 |
| unicode_1m / stringify | 1,442 | 139.594 / 140.331 / 413.633 / 449.318 | 0.99× | 57.17 / 57.28 / 158.53 / 125.86 |
| wide_1m / parse | 36 | 20,883.722 / 20,859.778 / 4,936.722 / 4,168.056 | 1.00× | 176.67 / 176.80 / 110.56 / 86.12 |
| wide_1m / stringify | 214 | 1,707.771 / 1,691.603 / 6,363.995 / 665.070 | 1.01× | 70.84 / 70.95 / 116.45 / 83.39 |
| heterogeneous_1m / parse | 81 | 1,909.840 / 1,911.802 / 3,962.506 / 2,961.210 | 1.00× | 69.39 / 69.50 / 92.14 / 80.81 |
| heterogeneous_1m / stringify | 210 | 1,351.800 / 843.352 / 908.614 / 1,054.114 | 1.60× | 59.14 / 59.25 / 128.75 / 103.14 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.17 / 12.42 / 52.58 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.80 / 25.02 / 74.78 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.30 / 12.55 / 52.67 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 24.89 / 25.14 / 71.16 / 47.02 |
| small_record / retain-parse | 1 | 12.19 / 12.45 / 52.69 / 28.33 |
| small_record / retain-parse | 100,000 | 44.98 / 45.12 / 85.45 / 52.69 |
| small_record / retain-stringify | 1 | 12.33 / 12.61 / 52.77 / 28.36 |
| small_record / retain-stringify | 100,000 | 28.09 / 28.33 / 80.31 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.41 / 16.62 / 58.41 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.59 / 41.81 / 88.70 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.20 / 19.44 / 60.52 / 34.44 |
| records_array_1m / retain-stringify | 16 | 33.08 / 33.34 / 76.30 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.08 / 15.31 / 58.33 / 31.61 |
| records_object_1m / retain-parse | 16 | 56.59 / 56.80 / 88.69 / 55.92 |
| records_object_1m / retain-stringify | 1 | 19.16 / 19.45 / 60.50 / 34.44 |
| records_object_1m / retain-stringify | 16 | 33.06 / 33.36 / 76.30 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.06 / 48.25 / 94.30 / 52.33 |
| records_array_8m / retain-parse | 4 | 86.45 / 86.61 / 146.86 / 92.05 |
| records_array_8m / retain-stringify | 1 | 72.84 / 73.05 / 114.14 / 73.45 |
| records_array_8m / retain-stringify | 4 | 91.58 / 91.78 / 144.00 / 94.47 |
| records_object_8m / retain-parse | 1 | 36.20 / 36.48 / 94.27 / 52.30 |
| records_object_8m / retain-parse | 4 | 97.53 / 97.72 / 146.86 / 91.83 |
| records_object_8m / retain-stringify | 1 | 72.84 / 73.06 / 114.19 / 73.47 |
| records_object_8m / retain-stringify | 4 | 91.58 / 91.80 / 143.91 / 94.48 |
| long_string_1m / retain-parse | 1 | 15.33 / 15.56 / 56.52 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.84 / 57.03 / 89.16 / 61.64 |
| long_string_1m / retain-stringify | 1 | 18.77 / 19.02 / 60.55 / 32.62 |
| long_string_1m / retain-stringify | 32 | 58.77 / 58.95 / 92.25 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.55 / 14.75 / 57.64 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.77 / 44.94 / 86.27 / 58.95 |
| unicode_1m / retain-stringify | 1 | 16.73 / 16.97 / 60.81 / 34.05 |
| unicode_1m / retain-stringify | 32 | 46.72 / 46.92 / 89.16 / 61.80 |
| wide_1m / retain-parse | 1 | 24.11 / 24.31 / 65.56 / 36.30 |
| wide_1m / retain-parse | 16 | 71.28 / 71.50 / 112.50 / 68.86 |
| wide_1m / retain-stringify | 1 | 30.14 / 30.38 / 68.58 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.41 / 45.64 / 86.42 / 53.47 |

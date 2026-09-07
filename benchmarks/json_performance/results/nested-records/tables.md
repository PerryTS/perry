Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.41 / 13.47 / 57.58 / 35.72 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.55 / 13.56 / 59.53 / 129.84 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.41 / 13.47 / 57.56 / 36.12 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.55 / 13.56 / 59.50 / 129.75 |
| empty_object / parse | 2,000,000 | 0.057 / 0.058 / 0.045 / 0.024 | 0.99× | 34.66 / 34.83 / 59.42 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.047 / 0.047 / 0.032 / 0.030 | 0.99× | 13.67 / 13.67 / 59.50 / 129.78 |
| tiny_object / parse | 2,000,000 | 0.120 / 0.121 / 0.083 / 0.046 | 0.99× | 34.72 / 34.84 / 59.47 / 69.06 |
| tiny_object / stringify | 2,000,000 | 0.091 / 0.091 / 0.037 / 0.039 | 1.00× | 34.69 / 34.83 / 59.56 / 129.75 |
| small_record / parse | 576,876 | 0.677 / 0.669 / 0.365 / 0.252 | 1.01× | 34.42 / 34.59 / 59.52 / 79.86 |
| small_record / stringify | 1,289,396 | 0.281 / 0.280 / 0.109 / 0.121 | 1.00× | 35.83 / 35.98 / 59.67 / 257.28 |
| object_1k / parse | 553,250 | 0.596 / 0.587 / 0.542 / 0.237 | 1.01× | 34.94 / 35.03 / 61.59 / 71.16 |
| object_1k / stringify | 662,860 | 0.233 / 0.234 / 0.198 / 0.215 | 1.00× | 35.83 / 35.97 / 61.66 / 70.83 |
| records_array_16k / parse | 6,703 | 23.787 / 23.824 / 39.274 / 33.950 | 1.00× | 64.84 / 65.06 / 65.70 / 70.41 |
| records_array_16k / stringify | 10,140 | 23.454 / 23.488 / 13.261 / 23.258 | 1.00× | 36.11 / 36.27 / 61.77 / 71.02 |
| records_array_1m / parse | 96 | 1,551.677 / 1,553.490 / 2,703.115 / 2,136.094 | 1.00× | 62.05 / 62.25 / 93.47 / 79.11 |
| records_array_1m / stringify | 176 | 1,493.841 / 1,514.188 / 843.080 / 970.972 | 0.99× | 59.31 / 59.52 / 108.28 / 100.33 |
| records_object_1m / parse | 70 | 3,904.200 / 3,920.471 / 2,878.100 / 2,137.986 | 1.00× | 58.61 / 58.72 / 92.69 / 75.92 |
| records_object_1m / stringify | 177 | 1,493.158 / 1,509.847 / 853.079 / 971.791 | 0.99× | 59.31 / 59.52 / 109.16 / 100.75 |
| records_array_8m / parse | 10 | 13,420.700 / 13,380.700 / 33,977.900 / 20,777.600 | 1.00× | 99.45 / 99.66 / 243.91 / 131.89 |
| records_array_8m / stringify | 20 | 12,992.950 / 13,113.600 / 6,811.150 / 8,527.000 | 0.99× | 193.61 / 193.86 / 185.91 / 183.23 |
| records_object_8m / parse | 8 | 30,309.625 / 30,463.250 / 30,324.500 / 21,792.250 | 0.99× | 166.70 / 166.84 / 225.52 / 111.52 |
| records_object_8m / stringify | 20 | 12,985.850 / 13,092.650 / 6,796.850 / 8,423.300 | 0.99× | 193.64 / 193.86 / 186.05 / 169.64 |
| records_array_20m / parse | 3 | 75,401.667 / 75,166.667 / 97,059.000 / 56,455.667 | 1.00× | 223.80 / 223.97 / 325.98 / 188.16 |
| records_array_20m / stringify | 8 | 80,142.625 / 81,301.250 / 17,345.750 / 21,072.000 | 0.99× | 158.70 / 158.88 / 392.19 / 304.73 |
| records_object_20m / parse | 3 | 75,073.000 / 75,135.667 / 95,775.333 / 56,935.000 | 1.00× | 223.80 / 223.95 / 325.89 / 188.06 |
| records_object_20m / stringify | 8 | 80,364.875 / 81,354.250 / 17,431.250 / 20,950.375 | 0.99× | 158.73 / 158.89 / 392.16 / 304.83 |
| numbers_1m / parse | 92 | 1,736.717 / 1,709.272 / 3,128.674 / 3,207.663 | 1.02× | 64.86 / 65.09 / 87.34 / 68.19 |
| numbers_1m / stringify | 96 | 1,605.688 / 1,603.760 / 1,980.188 / 2,952.146 | 1.00× | 61.27 / 61.45 / 92.17 / 72.23 |
| long_string_1m / parse | 1,346 | 204.663 / 204.999 / 368.418 / 66.169 | 1.00× | 60.73 / 60.94 / 160.97 / 124.44 |
| long_string_1m / stringify | 1,164 | 174.777 / 173.953 / 105.006 / 97.520 | 1.00× | 61.91 / 62.09 / 157.95 / 157.59 |
| escaped_1m / parse | 83 | 2,042.253 / 2,037.181 / 1,732.614 / 2,083.530 | 1.00× | 58.12 / 58.33 / 73.86 / 64.91 |
| escaped_1m / stringify | 169 | 949.982 / 964.432 / 1,892.740 / 2,086.225 | 0.99× | 57.09 / 57.28 / 124.61 / 74.73 |
| unicode_1m / parse | 1,440 | 256.276 / 256.595 / 438.031 / 60.326 | 1.00× | 55.44 / 55.64 / 164.03 / 156.22 |
| unicode_1m / stringify | 1,389 | 138.574 / 141.528 / 413.138 / 451.121 | 0.98× | 57.12 / 57.31 / 157.73 / 159.27 |
| wide_1m / parse | 36 | 20,967.222 / 20,861.250 / 4,970.806 / 4,165.444 | 1.01× | 176.64 / 176.80 / 110.55 / 86.16 |
| wide_1m / stringify | 216 | 1,705.296 / 1,691.046 / 6,345.731 / 665.787 | 1.01× | 70.86 / 71.05 / 116.47 / 84.45 |
| heterogeneous_1m / parse | 81 | 1,944.691 / 1,914.321 / 3,873.346 / 2,981.741 | 1.02× | 69.33 / 69.55 / 92.62 / 80.55 |
| heterogeneous_1m / stringify | 168 | 4,343.661 / 1,988.988 / 905.500 / 1,059.512 | 2.18× | 59.12 / 59.58 / 110.56 / 101.45 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.38 / 12.16 / 52.58 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.97 / 24.72 / 74.75 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.48 / 12.25 / 52.72 / 28.28 |
| tiny_object / retain-stringify | 200,000 | 25.06 / 24.81 / 71.17 / 47.02 |
| small_record / retain-parse | 1 | 12.38 / 12.17 / 52.67 / 28.30 |
| small_record / retain-parse | 100,000 | 45.11 / 45.03 / 85.42 / 52.67 |
| small_record / retain-stringify | 1 | 12.53 / 12.31 / 52.73 / 28.34 |
| small_record / retain-stringify | 100,000 | 28.23 / 28.03 / 80.25 / 51.06 |
| records_array_1m / retain-parse | 1 | 16.55 / 16.34 / 58.28 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.73 / 41.55 / 88.62 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.45 / 19.19 / 60.50 / 34.44 |
| records_array_1m / retain-stringify | 16 | 33.34 / 33.09 / 76.27 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.25 / 15.03 / 58.42 / 31.62 |
| records_object_1m / retain-parse | 16 | 56.73 / 56.69 / 88.64 / 55.92 |
| records_object_1m / retain-stringify | 1 | 19.42 / 19.19 / 60.56 / 34.41 |
| records_object_1m / retain-stringify | 16 | 33.34 / 33.11 / 76.33 / 47.23 |
| records_array_8m / retain-parse | 1 | 48.20 / 47.98 / 94.27 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.58 / 86.58 / 146.83 / 91.88 |
| records_array_8m / retain-stringify | 1 | 72.98 / 72.98 / 114.14 / 73.47 |
| records_array_8m / retain-stringify | 4 | 91.72 / 91.72 / 143.94 / 94.48 |
| records_object_8m / retain-parse | 1 | 36.38 / 36.08 / 94.30 / 52.30 |
| records_object_8m / retain-parse | 4 | 97.67 / 97.62 / 146.88 / 91.84 |
| records_object_8m / retain-stringify | 1 | 73.02 / 73.00 / 114.12 / 73.47 |
| records_object_8m / retain-stringify | 4 | 91.75 / 91.73 / 143.94 / 94.48 |
| long_string_1m / retain-parse | 1 | 15.47 / 15.30 / 56.48 / 30.61 |
| long_string_1m / retain-parse | 32 | 56.95 / 57.00 / 89.14 / 61.66 |
| long_string_1m / retain-stringify | 1 | 18.91 / 18.70 / 60.55 / 32.62 |
| long_string_1m / retain-stringify | 32 | 58.91 / 58.91 / 92.27 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.69 / 14.45 / 57.55 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.89 / 44.64 / 86.20 / 58.92 |
| unicode_1m / retain-stringify | 1 | 16.91 / 16.67 / 60.80 / 34.05 |
| unicode_1m / retain-stringify | 32 | 46.86 / 46.62 / 89.17 / 61.77 |
| wide_1m / retain-parse | 1 | 24.27 / 24.05 / 65.58 / 36.28 |
| wide_1m / retain-parse | 16 | 71.44 / 71.22 / 112.36 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.34 / 30.12 / 68.61 / 39.25 |
| wide_1m / retain-stringify | 16 | 45.73 / 45.39 / 86.41 / 53.45 |

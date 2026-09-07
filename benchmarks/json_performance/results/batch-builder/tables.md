Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.00× | 13.58 / 13.47 / 57.52 / 35.72 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.67 / 13.61 / 59.48 / 129.78 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.00× | 13.58 / 13.47 / 57.58 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.67 / 13.61 / 59.56 / 129.81 |
| empty_object / parse | 2,000,000 | 0.058 / 0.057 / 0.045 / 0.024 | 1.01× | 34.88 / 34.78 / 59.48 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.047 / 0.047 / 0.032 / 0.030 | 1.01× | 13.78 / 13.78 / 59.53 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.121 / 0.122 / 0.083 / 0.045 | 0.99× | 34.89 / 34.80 / 59.48 / 69.05 |
| tiny_object / stringify | 2,000,000 | 0.091 / 0.093 / 0.037 / 0.039 | 0.98× | 34.88 / 34.83 / 59.56 / 129.78 |
| small_record / parse | 579,290 | 0.671 / 0.542 / 0.367 / 0.252 | 1.24× | 34.64 / 34.55 / 59.55 / 79.86 |
| small_record / stringify | 1,270,960 | 0.280 / 0.283 / 0.108 / 0.121 | 0.99× | 36.02 / 35.98 / 59.59 / 254.16 |
| object_1k / parse | 535,833 | 0.593 / 0.543 / 0.544 / 0.238 | 1.09× | 35.12 / 35.00 / 61.72 / 71.12 |
| object_1k / stringify | 671,267 | 0.232 / 0.238 / 0.198 / 0.214 | 0.98× | 36.02 / 35.94 / 61.72 / 70.83 |
| records_array_16k / parse | 6,722 | 23.810 / 23.820 / 41.337 / 34.008 | 1.00× | 65.02 / 64.83 / 65.69 / 70.44 |
| records_array_16k / stringify | 10,098 | 23.545 / 23.349 / 13.236 / 23.181 | 1.01× | 36.30 / 36.27 / 61.78 / 71.03 |
| records_array_1m / parse | 96 | 1,553.573 / 1,553.010 / 2,810.552 / 2,133.667 | 1.00× | 62.23 / 62.02 / 93.47 / 79.11 |
| records_array_1m / stringify | 176 | 1,501.148 / 1,496.188 / 844.534 / 970.136 | 1.00× | 59.47 / 59.38 / 108.16 / 100.30 |
| records_object_1m / parse | 71 | 3,936.000 / 2,832.859 / 2,758.197 / 2,138.873 | 1.39× | 60.31 / 60.20 / 92.75 / 75.88 |
| records_object_1m / stringify | 177 | 1,496.718 / 1,494.367 / 847.435 / 970.944 | 1.00× | 59.47 / 59.41 / 109.05 / 100.33 |
| records_array_8m / parse | 10 | 13,393.800 / 13,400.600 / 33,798.000 / 20,983.600 | 1.00× | 99.62 / 99.42 / 244.00 / 131.28 |
| records_array_8m / stringify | 21 | 12,935.524 / 12,882.333 / 6,804.381 / 8,482.952 | 1.00× | 193.78 / 193.77 / 186.08 / 176.59 |
| records_object_8m / parse | 8 | 30,698.500 / 21,600.500 / 29,557.750 / 23,925.000 | 1.42× | 166.86 / 166.86 / 225.38 / 111.50 |
| records_object_8m / stringify | 21 | 12,993.429 / 12,897.048 / 6,821.048 / 8,397.476 | 1.01× | 193.80 / 193.83 / 186.02 / 176.48 |
| records_array_20m / parse | 3 | 75,687.667 / 53,281.000 / 95,753.667 / 56,863.000 | 1.42× | 223.97 / 224.28 / 326.03 / 187.97 |
| records_array_20m / stringify | 8 | 81,689.000 / 81,993.000 / 17,477.250 / 20,842.250 | 1.00× | 158.86 / 158.73 / 392.14 / 304.88 |
| records_object_20m / parse | 3 | 75,763.333 / 53,417.667 / 94,211.000 / 56,437.333 | 1.42× | 223.97 / 224.28 / 326.27 / 187.66 |
| records_object_20m / stringify | 8 | 81,623.625 / 82,049.250 / 17,347.500 / 20,725.625 | 0.99× | 158.88 / 158.78 / 392.11 / 304.92 |
| numbers_1m / parse | 91 | 1,712.747 / 1,714.604 / 3,119.604 / 3,207.330 | 1.00× | 65.05 / 64.86 / 86.20 / 67.16 |
| numbers_1m / stringify | 96 | 1,604.906 / 1,608.812 / 1,982.844 / 2,948.927 | 1.00× | 61.39 / 63.48 / 92.23 / 72.22 |
| long_string_1m / parse | 1,335 | 204.522 / 205.440 / 368.844 / 66.024 | 1.00× | 60.89 / 60.73 / 160.97 / 124.44 |
| long_string_1m / stringify | 1,171 | 173.165 / 171.895 / 102.611 / 97.276 | 1.01× | 62.05 / 61.94 / 158.95 / 153.58 |
| escaped_1m / parse | 82 | 2,040.573 / 2,043.561 / 1,731.756 / 2,083.707 | 1.00× | 58.28 / 58.12 / 73.95 / 64.19 |
| escaped_1m / stringify | 169 | 951.757 / 949.722 / 1,891.154 / 2,097.065 | 1.00× | 57.27 / 57.16 / 124.59 / 74.78 |
| unicode_1m / parse | 1,466 | 258.999 / 256.206 / 437.824 / 60.216 | 1.01× | 55.61 / 55.45 / 164.94 / 151.73 |
| unicode_1m / stringify | 1,390 | 137.640 / 139.042 / 413.120 / 450.913 | 0.99× | 57.28 / 57.17 / 157.69 / 126.31 |
| wide_1m / parse | 36 | 20,890.111 / 20,390.111 / 4,957.639 / 4,151.639 | 1.02× | 176.80 / 176.66 / 110.56 / 86.11 |
| wide_1m / stringify | 212 | 1,693.656 / 1,691.858 / 6,336.019 / 667.250 | 1.00× | 70.95 / 70.98 / 116.50 / 84.36 |
| heterogeneous_1m / parse | 81 | 1,916.346 / 1,932.185 / 3,855.259 / 2,983.247 | 0.99× | 69.52 / 69.33 / 92.52 / 80.78 |
| heterogeneous_1m / stringify | 211 | 841.379 / 844.517 / 897.640 / 1,053.512 | 1.00× | 59.25 / 59.22 / 128.83 / 103.11 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.42 / 12.14 / 52.62 / 28.30 |
| tiny_object / retain-parse | 200,000 | 25.02 / 24.70 / 74.67 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.55 / 12.31 / 52.70 / 28.33 |
| tiny_object / retain-stringify | 200,000 | 25.14 / 24.84 / 71.16 / 47.05 |
| small_record / retain-parse | 1 | 12.47 / 12.17 / 52.67 / 28.28 |
| small_record / retain-parse | 100,000 | 45.14 / 44.89 / 85.42 / 52.69 |
| small_record / retain-stringify | 1 | 12.61 / 12.36 / 52.73 / 28.34 |
| small_record / retain-stringify | 100,000 | 28.33 / 28.06 / 80.33 / 51.08 |
| records_array_1m / retain-parse | 1 | 16.62 / 16.33 / 58.38 / 31.61 |
| records_array_1m / retain-parse | 16 | 41.81 / 41.53 / 88.59 / 55.91 |
| records_array_1m / retain-stringify | 1 | 19.47 / 19.23 / 60.58 / 34.45 |
| records_array_1m / retain-stringify | 16 | 33.36 / 33.14 / 76.33 / 47.25 |
| records_object_1m / retain-parse | 1 | 15.31 / 15.02 / 58.41 / 31.62 |
| records_object_1m / retain-parse | 16 | 56.80 / 56.55 / 88.52 / 55.89 |
| records_object_1m / retain-stringify | 1 | 19.45 / 19.28 / 60.52 / 34.44 |
| records_object_1m / retain-stringify | 16 | 33.34 / 33.17 / 76.28 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.28 / 47.97 / 94.22 / 52.33 |
| records_array_8m / retain-parse | 4 | 86.62 / 86.30 / 146.92 / 92.06 |
| records_array_8m / retain-stringify | 1 | 73.00 / 72.89 / 114.20 / 73.44 |
| records_array_8m / retain-stringify | 4 | 91.73 / 91.62 / 143.91 / 94.48 |
| records_object_8m / retain-parse | 1 | 36.45 / 36.19 / 94.25 / 52.33 |
| records_object_8m / retain-parse | 4 | 97.69 / 97.48 / 146.88 / 91.81 |
| records_object_8m / retain-stringify | 1 | 73.02 / 72.94 / 114.16 / 73.47 |
| records_object_8m / retain-stringify | 4 | 91.75 / 91.67 / 143.59 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.53 / 15.25 / 56.50 / 30.58 |
| long_string_1m / retain-parse | 32 | 56.98 / 56.70 / 89.08 / 61.64 |
| long_string_1m / retain-stringify | 1 | 18.97 / 18.72 / 60.52 / 32.62 |
| long_string_1m / retain-stringify | 32 | 58.95 / 58.67 / 92.23 / 63.75 |
| unicode_1m / retain-parse | 1 | 14.73 / 14.41 / 57.70 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.94 / 44.61 / 86.28 / 58.95 |
| unicode_1m / retain-stringify | 1 | 16.97 / 16.67 / 60.77 / 34.11 |
| unicode_1m / retain-stringify | 32 | 46.92 / 46.62 / 89.12 / 61.75 |
| wide_1m / retain-parse | 1 | 24.33 / 23.98 / 65.66 / 36.33 |
| wide_1m / retain-parse | 16 | 71.50 / 71.17 / 112.45 / 68.89 |
| wide_1m / retain-stringify | 1 | 30.38 / 30.19 / 68.64 / 39.20 |
| wide_1m / retain-stringify | 16 | 45.64 / 45.45 / 86.44 / 53.47 |

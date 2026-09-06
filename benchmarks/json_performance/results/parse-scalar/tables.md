Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.251 / 0.013 / 0.027 / 0.019 | 20.06× | 13.55 / 13.44 / 57.58 / 35.69 |
| null / stringify | 2,000,000 | 0.066 / 0.007 / 0.027 / 0.029 | 10.07× | 34.89 / 13.61 / 59.53 / 129.75 |
| string_a / parse | 2,000,000 | 0.254 / 0.015 / 0.032 / 0.023 | 16.96× | 13.56 / 13.44 / 57.59 / 36.06 |
| string_a / stringify | 2,000,000 | 0.075 / 0.010 / 0.030 / 0.029 | 7.70× | 34.86 / 13.61 / 59.55 / 129.72 |
| empty_object / parse | 2,000,000 | 0.326 / 0.329 / 0.045 / 0.025 | 0.99× | 34.62 / 34.52 / 59.47 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.137 / 0.051 / 0.032 / 0.030 | 2.67× | 34.92 / 13.64 / 59.55 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.364 / 0.369 / 0.081 / 0.045 | 0.99× | 34.64 / 34.53 / 59.48 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.197 / 0.140 / 0.037 / 0.040 | 1.41× | 34.92 / 34.75 / 59.64 / 129.73 |
| small_record / parse | 570,658 | 0.753 / 0.750 / 0.334 / 0.253 | 1.00× | 35.02 / 34.91 / 59.45 / 79.88 |
| small_record / stringify | 1,279,317 | 0.550 / 0.547 / 0.109 / 0.121 | 1.01× | 35.98 / 35.86 / 59.64 / 255.55 |
| object_1k / parse | 541,964 | 0.594 / 0.595 / 0.543 / 0.238 | 1.00× | 35.03 / 34.95 / 61.70 / 71.12 |
| object_1k / stringify | 644,295 | 0.386 / 0.291 / 0.199 / 0.217 | 1.33× | 36.08 / 35.88 / 61.70 / 70.84 |
| records_array_16k / parse | 6,697 | 23.858 / 23.871 / 38.628 / 34.106 | 1.00× | 65.02 / 64.84 / 65.73 / 70.42 |
| records_array_16k / stringify | 10,070 | 35.872 / 31.734 / 13.248 / 23.324 | 1.13× | 36.28 / 36.14 / 61.80 / 70.98 |
| records_array_1m / parse | 93 | 1,542.968 / 1,517.043 / 2,758.935 / 2,147.935 | 1.02× | 62.31 / 62.12 / 92.78 / 79.12 |
| records_array_1m / stringify | 175 | 2,204.880 / 1,979.349 / 855.966 / 980.486 | 1.11× | 60.56 / 60.41 / 107.55 / 100.30 |
| records_object_1m / parse | 70 | 4,038.071 / 4,018.729 / 2,928.814 / 2,143.300 | 1.00× | 80.14 / 80.02 / 92.75 / 75.92 |
| records_object_1m / stringify | 172 | 2,217.965 / 1,999.837 / 854.988 / 974.023 | 1.11× | 60.55 / 60.39 / 104.95 / 100.31 |
| records_array_8m / parse | 10 | 13,864.500 / 13,680.400 / 34,285.900 / 20,949.300 | 1.01× | 102.75 / 102.58 / 244.09 / 131.19 |
| records_array_8m / stringify | 20 | 18,773.400 / 16,859.750 / 6,864.000 / 8,555.150 | 1.11× | 196.03 / 195.92 / 186.12 / 169.80 |
| records_object_8m / parse | 8 | 31,770.375 / 31,713.000 / 30,501.125 / 21,956.375 | 1.00× | 231.81 / 231.66 / 225.47 / 111.56 |
| records_object_8m / stringify | 20 | 18,748.700 / 16,873.350 / 6,839.700 / 8,520.600 | 1.11× | 196.05 / 195.92 / 186.05 / 169.69 |
| records_array_20m / parse | 3 | 78,780.667 / 79,002.333 / 98,555.000 / 57,372.333 | 1.00× | 315.81 / 315.64 / 325.88 / 187.95 |
| records_array_20m / stringify | 8 | 97,673.750 / 93,259.750 / 17,506.375 / 20,867.750 | 1.05× | 171.38 / 171.14 / 392.28 / 304.88 |
| records_object_20m / parse | 3 | 78,907.667 / 78,208.000 / 98,120.000 / 57,319.000 | 1.01× | 315.83 / 315.64 / 326.00 / 187.98 |
| records_object_20m / stringify | 8 | 97,764.625 / 93,113.000 / 17,408.750 / 20,862.875 | 1.05× | 171.36 / 171.16 / 392.12 / 304.70 |
| numbers_1m / parse | 93 | 1,565.935 / 1,694.613 / 3,128.882 / 3,214.903 | 0.92× | 64.55 / 64.44 / 88.20 / 68.14 |
| numbers_1m / stringify | 95 | 6,596.926 / 1,607.726 / 1,986.811 / 2,957.242 | 4.10× | 61.44 / 61.25 / 91.22 / 72.20 |
| long_string_1m / parse | 1,220 | 203.174 / 205.505 / 370.590 / 66.900 | 0.99× | 60.91 / 60.77 / 156.97 / 124.38 |
| long_string_1m / stringify | 1,112 | 206.327 / 172.123 / 108.126 / 98.865 | 1.20× | 64.02 / 61.95 / 156.88 / 159.58 |
| escaped_1m / parse | 82 | 2,040.732 / 2,044.915 / 1,734.854 / 2,091.634 | 1.00× | 58.28 / 58.16 / 73.98 / 64.20 |
| escaped_1m / stringify | 83 | 1,864.482 / 1,905.952 / 1,868.639 / 2,104.880 | 0.98× | 57.08 / 56.98 / 79.48 / 68.98 |
| unicode_1m / parse | 1,381 | 257.091 / 256.558 / 439.238 / 61.718 | 1.00× | 55.64 / 55.45 / 163.23 / 157.91 |
| unicode_1m / stringify | 1,393 | 253.790 / 138.352 / 414.900 / 452.284 | 1.83× | 58.41 / 57.19 / 157.78 / 131.16 |
| wide_1m / parse | 36 | 21,108.306 / 20,766.917 / 4,977.361 / 4,185.306 | 1.02× | 176.77 / 176.66 / 110.58 / 86.11 |
| wide_1m / stringify | 214 | 2,474.598 / 2,466.579 / 6,359.879 / 670.042 | 1.00× | 71.00 / 70.84 / 116.55 / 84.67 |
| heterogeneous_1m / parse | 77 | 1,874.325 / 1,915.208 / 3,958.260 / 2,985.091 | 0.98× | 68.19 / 68.03 / 92.59 / 80.80 |
| heterogeneous_1m / stringify | 167 | 4,487.910 / 4,529.898 / 919.647 / 1,063.491 | 0.99× | 59.27 / 59.11 / 110.53 / 101.41 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.33 / 12.28 / 52.62 / 28.27 |
| tiny_object / retain-parse | 200,000 | 24.91 / 24.86 / 74.75 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.41 / 52.70 / 28.30 |
| tiny_object / retain-stringify | 200,000 | 36.41 / 25.00 / 71.14 / 47.02 |
| small_record / retain-parse | 1 | 12.34 / 12.30 / 52.67 / 28.30 |
| small_record / retain-parse | 100,000 | 57.00 / 56.78 / 85.44 / 52.69 |
| small_record / retain-stringify | 1 | 12.61 / 12.52 / 52.72 / 28.34 |
| small_record / retain-stringify | 100,000 | 31.44 / 31.34 / 80.38 / 51.06 |
| records_array_1m / retain-parse | 1 | 17.97 / 17.88 / 58.38 / 31.66 |
| records_array_1m / retain-parse | 16 | 41.38 / 41.30 / 88.73 / 55.88 |
| records_array_1m / retain-stringify | 1 | 20.17 / 20.06 / 60.64 / 34.45 |
| records_array_1m / retain-stringify | 16 | 34.06 / 33.95 / 76.39 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.98 / 15.97 / 58.41 / 31.61 |
| records_object_1m / retain-parse | 16 | 67.14 / 66.95 / 88.80 / 55.89 |
| records_object_1m / retain-stringify | 1 | 20.16 / 20.08 / 60.53 / 34.45 |
| records_object_1m / retain-stringify | 16 | 34.06 / 33.98 / 76.27 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.61 / 48.52 / 94.20 / 52.30 |
| records_array_8m / retain-parse | 4 | 89.30 / 89.09 / 146.91 / 91.67 |
| records_array_8m / retain-stringify | 1 | 75.77 / 75.50 / 114.19 / 73.41 |
| records_array_8m / retain-stringify | 4 | 95.06 / 94.80 / 143.95 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.95 / 42.97 / 94.22 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.44 / 113.25 / 146.97 / 91.78 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.52 / 114.09 / 73.45 |
| records_object_8m / retain-stringify | 4 | 95.06 / 94.81 / 143.94 / 94.53 |
| long_string_1m / retain-parse | 1 | 15.45 / 15.42 / 56.47 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.98 / 56.80 / 89.14 / 61.62 |
| long_string_1m / retain-stringify | 1 | 21.14 / 18.88 / 60.48 / 32.66 |
| long_string_1m / retain-stringify | 32 | 59.89 / 58.75 / 92.23 / 63.70 |
| unicode_1m / retain-parse | 1 | 14.64 / 14.59 / 57.59 / 31.23 |
| unicode_1m / retain-parse | 32 | 44.84 / 44.80 / 86.31 / 58.94 |
| unicode_1m / retain-stringify | 1 | 18.73 / 16.81 / 60.84 / 34.05 |
| unicode_1m / retain-stringify | 32 | 48.69 / 46.77 / 89.12 / 61.77 |
| wide_1m / retain-parse | 1 | 24.20 / 24.17 / 65.59 / 36.30 |
| wide_1m / retain-parse | 16 | 71.38 / 71.36 / 112.48 / 68.84 |
| wide_1m / retain-stringify | 1 | 30.28 / 30.23 / 68.56 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.55 / 45.50 / 86.48 / 53.44 |

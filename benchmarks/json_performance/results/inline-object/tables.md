Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.013 / 0.013 / 0.027 / 0.019 | 1.02× | 13.58 / 13.45 / 57.50 / 35.72 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.66 / 13.55 / 59.52 / 129.80 |
| string_a / parse | 2,000,000 | 0.016 / 0.016 / 0.032 / 0.023 | 1.02× | 13.58 / 13.45 / 57.62 / 36.09 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.66 / 13.55 / 59.62 / 129.78 |
| empty_object / parse | 2,000,000 | 0.057 / 0.056 / 0.045 / 0.024 | 1.01× | 34.80 / 34.66 / 59.45 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.047 / 0.048 / 0.032 / 0.030 | 0.99× | 13.73 / 13.62 / 59.53 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.363 / 0.120 / 0.082 / 0.045 | 3.04× | 34.62 / 34.67 / 59.50 / 69.05 |
| tiny_object / stringify | 2,000,000 | 0.091 / 0.091 / 0.037 / 0.040 | 1.00× | 34.81 / 34.67 / 59.59 / 129.73 |
| small_record / parse | 577,987 | 0.683 / 0.679 / 0.350 / 0.252 | 1.01× | 35.00 / 34.83 / 59.50 / 79.86 |
| small_record / stringify | 1,294,733 | 0.282 / 0.281 / 0.109 / 0.121 | 1.00× | 35.95 / 35.81 / 59.61 / 258.16 |
| object_1k / parse | 556,886 | 0.598 / 0.591 / 0.541 / 0.237 | 1.01× | 35.05 / 34.91 / 61.70 / 71.12 |
| object_1k / stringify | 644,237 | 0.234 / 0.235 / 0.199 / 0.216 | 1.00× | 35.95 / 35.81 / 61.67 / 70.81 |
| records_array_16k / parse | 6,648 | 23.801 / 24.021 / 40.154 / 33.998 | 0.99× | 65.08 / 64.89 / 65.73 / 70.39 |
| records_array_16k / stringify | 9,729 | 23.492 / 23.223 / 13.243 / 23.306 | 1.01× | 36.25 / 36.08 / 61.75 / 71.02 |
| records_array_1m / parse | 95 | 1,552.547 / 1,561.600 / 2,700.032 / 2,132.800 | 0.99× | 62.28 / 62.06 / 92.73 / 79.11 |
| records_array_1m / stringify | 174 | 1,502.730 / 1,498.109 / 849.943 / 970.339 | 1.00× | 60.58 / 60.41 / 106.53 / 100.30 |
| records_object_1m / parse | 70 | 4,029.671 / 4,041.571 / 2,829.857 / 2,131.543 | 1.00× | 80.14 / 79.97 / 92.67 / 75.91 |
| records_object_1m / stringify | 178 | 1,503.528 / 1,496.775 / 844.697 / 969.758 | 1.00× | 60.58 / 60.39 / 109.94 / 100.31 |
| records_array_8m / parse | 10 | 13,397.600 / 13,485.100 / 33,649.100 / 20,854.900 | 0.99× | 99.67 / 99.47 / 243.98 / 131.39 |
| records_array_8m / stringify | 21 | 13,068.048 / 13,013.524 / 6,813.286 / 8,430.381 | 1.00× | 196.11 / 195.94 / 186.00 / 176.28 |
| records_object_8m / parse | 8 | 31,408.625 / 31,373.375 / 29,720.625 / 21,686.375 | 1.00× | 231.91 / 231.70 / 225.41 / 111.70 |
| records_object_8m / stringify | 20 | 13,120.300 / 13,112.850 / 6,784.400 / 8,440.400 | 1.00× | 196.14 / 195.94 / 185.94 / 169.62 |
| records_array_20m / parse | 3 | 77,740.667 / 77,953.333 / 94,692.000 / 52,766.667 | 1.00× | 315.92 / 315.69 / 326.09 / 219.36 |
| records_array_20m / stringify | 8 | 85,073.625 / 84,945.250 / 17,405.750 / 20,955.625 | 1.00× | 171.42 / 171.19 / 392.19 / 304.84 |
| records_object_20m / parse | 3 | 78,047.000 / 77,454.000 / 96,901.667 / 57,057.333 | 1.01× | 315.92 / 315.69 / 325.89 / 188.09 |
| records_object_20m / stringify | 8 | 85,127.250 / 85,125.250 / 17,370.250 / 20,918.375 | 1.00× | 171.42 / 171.19 / 392.12 / 304.78 |
| numbers_1m / parse | 92 | 1,727.196 / 1,713.304 / 3,125.793 / 3,202.293 | 1.01× | 65.08 / 64.91 / 87.34 / 68.19 |
| numbers_1m / stringify | 96 | 1,609.656 / 1,602.823 / 1,982.688 / 2,951.000 | 1.00× | 61.47 / 61.27 / 92.22 / 72.25 |
| long_string_1m / parse | 1,314 | 206.012 / 204.075 / 368.631 / 67.572 | 1.01× | 61.98 / 60.78 / 159.95 / 158.39 |
| long_string_1m / stringify | 1,164 | 174.855 / 175.766 / 105.808 / 96.014 | 0.99× | 62.11 / 61.95 / 158.02 / 123.55 |
| escaped_1m / parse | 82 | 2,043.683 / 2,041.646 / 1,734.012 / 2,083.963 | 1.00× | 58.34 / 58.17 / 73.81 / 64.16 |
| escaped_1m / stringify | 168 | 951.631 / 968.107 / 1,899.643 / 2,086.286 | 0.98× | 57.33 / 57.16 / 124.61 / 74.77 |
| unicode_1m / parse | 1,420 | 258.253 / 258.573 / 438.204 / 59.454 | 1.00× | 56.55 / 55.50 / 164.00 / 123.12 |
| unicode_1m / stringify | 1,386 | 139.165 / 139.437 / 412.143 / 451.198 | 1.00× | 57.34 / 57.19 / 157.73 / 131.19 |
| wide_1m / parse | 36 | 20,892.500 / 20,956.222 / 4,954.694 / 4,166.556 | 1.00× | 176.84 / 176.69 / 110.61 / 86.12 |
| wide_1m / stringify | 214 | 1,694.262 / 1,691.995 / 6,340.355 / 665.710 | 1.00× | 72.03 / 70.88 / 116.55 / 84.27 |
| heterogeneous_1m / parse | 80 | 1,936.675 / 1,940.812 / 3,924.225 / 2,998.875 | 1.00× | 69.56 / 69.36 / 92.19 / 80.86 |
| heterogeneous_1m / stringify | 168 | 4,380.655 / 4,358.304 / 904.869 / 1,061.690 | 1.01× | 60.16 / 59.11 / 110.52 / 100.52 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.30 / 12.20 / 52.58 / 28.33 |
| tiny_object / retain-parse | 200,000 | 24.88 / 24.77 / 74.80 / 50.22 |
| tiny_object / retain-stringify | 1 | 12.39 / 12.31 / 52.73 / 28.31 |
| tiny_object / retain-stringify | 200,000 | 24.97 / 24.88 / 71.23 / 47.03 |
| small_record / retain-parse | 1 | 12.31 / 12.20 / 52.67 / 28.28 |
| small_record / retain-parse | 100,000 | 56.98 / 56.81 / 85.44 / 52.70 |
| small_record / retain-stringify | 1 | 12.45 / 12.34 / 52.77 / 28.38 |
| small_record / retain-stringify | 100,000 | 28.14 / 28.05 / 80.28 / 51.05 |
| records_array_1m / retain-parse | 1 | 16.45 / 16.34 / 58.44 / 31.62 |
| records_array_1m / retain-parse | 16 | 41.64 / 41.53 / 88.61 / 55.92 |
| records_array_1m / retain-stringify | 1 | 20.16 / 19.97 / 60.53 / 34.45 |
| records_array_1m / retain-stringify | 16 | 34.03 / 33.86 / 76.42 / 47.23 |
| records_object_1m / retain-parse | 1 | 15.98 / 15.86 / 58.33 / 31.64 |
| records_object_1m / retain-parse | 16 | 67.09 / 66.94 / 88.61 / 55.92 |
| records_object_1m / retain-stringify | 1 | 20.12 / 19.97 / 60.58 / 34.50 |
| records_object_1m / retain-stringify | 16 | 34.00 / 33.86 / 76.38 / 47.27 |
| records_array_8m / retain-parse | 1 | 48.11 / 47.97 / 94.20 / 52.31 |
| records_array_8m / retain-parse | 4 | 86.59 / 86.39 / 146.89 / 91.81 |
| records_array_8m / retain-stringify | 1 | 75.77 / 75.55 / 114.12 / 73.45 |
| records_array_8m / retain-stringify | 4 | 95.06 / 94.84 / 143.98 / 94.48 |
| records_object_8m / retain-parse | 1 | 42.94 / 42.86 / 94.22 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.45 / 113.30 / 146.94 / 91.98 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.56 / 114.14 / 73.48 |
| records_object_8m / retain-stringify | 4 | 95.06 / 94.86 / 143.98 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.31 / 15.33 / 56.44 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.97 / 56.84 / 89.11 / 61.66 |
| long_string_1m / retain-stringify | 1 | 18.80 / 18.75 / 60.53 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.91 / 58.77 / 92.19 / 63.73 |
| unicode_1m / retain-parse | 1 | 14.61 / 14.48 / 57.59 / 31.27 |
| unicode_1m / retain-parse | 32 | 44.81 / 44.69 / 86.22 / 58.95 |
| unicode_1m / retain-stringify | 1 | 16.80 / 16.72 / 60.86 / 34.08 |
| unicode_1m / retain-stringify | 32 | 46.77 / 46.66 / 89.12 / 61.77 |
| wide_1m / retain-parse | 1 | 24.20 / 24.06 / 65.64 / 36.31 |
| wide_1m / retain-parse | 16 | 71.38 / 71.25 / 112.47 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.23 / 30.09 / 68.62 / 39.23 |
| wide_1m / retain-stringify | 16 | 45.50 / 45.36 / 86.42 / 53.45 |

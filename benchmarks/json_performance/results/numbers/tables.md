Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.251 / 0.251 / 0.027 / 0.019 | 1.00× | 13.52 / 13.56 / 57.53 / 35.64 |
| null / stringify | 2,000,000 | 0.064 / 0.064 / 0.027 / 0.029 | 0.99× | 34.91 / 34.94 / 59.52 / 129.73 |
| string_a / parse | 2,000,000 | 0.256 / 0.255 / 0.032 / 0.023 | 1.00× | 13.52 / 13.56 / 57.55 / 36.05 |
| string_a / stringify | 2,000,000 | 0.072 / 0.072 / 0.030 / 0.029 | 1.00× | 34.86 / 34.91 / 59.59 / 129.72 |
| empty_object / parse | 2,000,000 | 0.326 / 0.326 / 0.045 / 0.024 | 1.00× | 34.66 / 34.69 / 59.45 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.135 / 0.135 / 0.032 / 0.030 | 0.99× | 34.95 / 34.98 / 59.52 / 129.75 |
| tiny_object / parse | 2,000,000 | 0.364 / 0.364 / 0.081 / 0.045 | 1.00× | 34.64 / 34.69 / 59.48 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.197 / 0.196 / 0.037 / 0.039 | 1.00× | 34.95 / 34.97 / 59.59 / 129.73 |
| small_record / parse | 587,947 | 0.748 / 0.756 / 0.351 / 0.252 | 0.99× | 35.05 / 35.06 / 59.64 / 79.81 |
| small_record / stringify | 1,274,783 | 0.547 / 0.550 / 0.109 / 0.120 | 0.99× | 36.03 / 36.06 / 59.69 / 254.75 |
| object_1k / parse | 525,279 | 0.590 / 0.589 / 0.544 / 0.238 | 1.00× | 35.08 / 35.06 / 61.66 / 71.11 |
| object_1k / stringify | 643,776 | 0.384 / 0.386 / 0.199 / 0.215 | 0.99× | 36.09 / 36.14 / 61.78 / 70.83 |
| records_array_16k / parse | 6,666 | 23.784 / 23.826 / 40.221 / 33.943 | 1.00× | 65.03 / 65.06 / 65.75 / 70.39 |
| records_array_16k / stringify | 10,027 | 36.158 / 35.959 / 13.311 / 23.229 | 1.01× | 36.38 / 36.45 / 61.78 / 70.98 |
| records_array_1m / parse | 93 | 1,539.903 / 1,540.140 / 2,691.000 / 2,140.011 | 1.00× | 62.30 / 62.31 / 92.81 / 79.14 |
| records_array_1m / stringify | 177 | 2,236.910 / 2,210.927 / 844.124 / 969.424 | 1.01× | 61.62 / 61.70 / 109.19 / 100.25 |
| records_object_1m / parse | 71 | 4,032.197 / 4,063.648 / 2,717.521 / 2,137.507 | 0.99× | 80.45 / 80.48 / 92.73 / 75.86 |
| records_object_1m / stringify | 177 | 2,229.073 / 2,210.921 / 848.870 / 969.418 | 1.01× | 61.62 / 61.70 / 109.16 / 100.25 |
| records_array_8m / parse | 10 | 13,796.700 / 13,793.500 / 34,028.500 / 21,002.000 | 1.00× | 102.78 / 102.80 / 244.02 / 131.20 |
| records_array_8m / stringify | 21 | 18,786.476 / 18,638.619 / 6,787.762 / 8,472.619 | 1.01× | 196.05 / 196.12 / 186.00 / 196.80 |
| records_object_8m / parse | 8 | 31,426.750 / 31,685.875 / 31,435.125 / 21,551.750 | 0.99× | 231.72 / 231.75 / 225.47 / 111.34 |
| records_object_8m / stringify | 21 | 18,736.333 / 18,662.095 / 6,756.952 / 8,454.524 | 1.00× | 196.05 / 196.12 / 186.05 / 196.28 |
| records_array_20m / parse | 3 | 78,068.333 / 78,475.000 / 94,457.000 / 56,793.667 | 0.99× | 315.67 / 315.73 / 326.00 / 187.77 |
| records_array_20m / stringify | 8 | 97,680.375 / 97,729.625 / 17,251.500 / 21,077.125 | 1.00× | 176.20 / 176.28 / 392.12 / 304.80 |
| records_object_20m / parse | 3 | 77,740.333 / 78,457.000 / 96,700.000 / 56,810.333 | 0.99× | 315.69 / 315.73 / 325.86 / 187.62 |
| records_object_20m / stringify | 8 | 97,923.625 / 97,475.625 / 17,325.750 / 20,820.375 | 1.00× | 176.19 / 176.27 / 392.16 / 304.75 |
| numbers_1m / parse | 96 | 1,574.833 / 1,576.375 / 3,119.927 / 3,210.562 | 1.00× | 64.83 / 64.58 / 91.16 / 68.14 |
| numbers_1m / stringify | 75 | 7,907.987 / 6,582.800 / 1,932.867 / 2,953.507 | 1.20× | 60.48 / 61.55 / 81.66 / 70.19 |
| long_string_1m / parse | 1,291 | 205.665 / 205.343 / 368.586 / 67.471 | 1.00× | 62.95 / 62.98 / 158.81 / 153.39 |
| long_string_1m / stringify | 1,058 | 204.946 / 206.957 / 106.689 / 97.995 | 0.99× | 65.06 / 65.09 / 154.92 / 155.50 |
| escaped_1m / parse | 82 | 2,040.732 / 2,039.537 / 1,735.220 / 2,082.476 | 1.00× | 58.31 / 58.34 / 73.84 / 64.16 |
| escaped_1m / stringify | 83 | 1,904.855 / 1,864.530 / 1,861.494 / 2,094.554 | 1.02× | 57.11 / 57.12 / 79.55 / 69.00 |
| unicode_1m / parse | 1,401 | 985.842 / 982.690 / 438.093 / 60.368 | 1.00× | 56.50 / 56.50 / 163.02 / 151.69 |
| unicode_1m / stringify | 331 | 983.076 / 982.453 / 436.686 / 455.631 | 1.00× | 59.19 / 59.20 / 129.39 / 80.92 |
| wide_1m / parse | 36 | 44,393.083 / 44,346.611 / 4,939.444 / 4,164.250 | 1.00× | 226.89 / 226.95 / 110.52 / 86.06 |
| wide_1m / stringify | 214 | 2,512.107 / 2,518.626 / 6,361.542 / 666.154 | 1.00× | 69.83 / 69.86 / 116.48 / 84.53 |
| heterogeneous_1m / parse | 78 | 1,869.308 / 1,870.628 / 3,830.038 / 2,965.897 | 1.00× | 68.08 / 68.11 / 92.20 / 80.69 |
| heterogeneous_1m / stringify | 168 | 4,507.155 / 4,523.714 / 908.815 / 1,059.565 | 1.00× | 60.11 / 60.14 / 110.61 / 101.52 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.28 / 12.33 / 52.58 / 28.28 |
| tiny_object / retain-parse | 200,000 | 24.81 / 24.84 / 74.86 / 50.19 |
| tiny_object / retain-stringify | 1 | 12.53 / 12.56 / 52.72 / 28.28 |
| tiny_object / retain-stringify | 200,000 | 36.39 / 36.39 / 71.17 / 47.00 |
| small_record / retain-parse | 1 | 12.31 / 12.34 / 52.69 / 28.27 |
| small_record / retain-parse | 100,000 | 57.00 / 57.00 / 85.42 / 52.66 |
| small_record / retain-stringify | 1 | 12.59 / 12.64 / 52.81 / 28.30 |
| small_record / retain-stringify | 100,000 | 31.38 / 31.41 / 80.31 / 51.03 |
| records_array_1m / retain-parse | 1 | 17.92 / 17.97 / 58.33 / 31.64 |
| records_array_1m / retain-parse | 16 | 41.33 / 41.38 / 88.69 / 55.86 |
| records_array_1m / retain-stringify | 1 | 20.12 / 20.16 / 60.45 / 34.45 |
| records_array_1m / retain-stringify | 16 | 34.02 / 34.05 / 76.36 / 47.20 |
| records_object_1m / retain-parse | 1 | 15.94 / 15.97 / 58.28 / 31.59 |
| records_object_1m / retain-parse | 16 | 67.11 / 67.09 / 88.62 / 55.88 |
| records_object_1m / retain-stringify | 1 | 20.14 / 20.20 / 60.55 / 34.39 |
| records_object_1m / retain-stringify | 16 | 34.05 / 34.09 / 76.31 / 47.20 |
| records_array_8m / retain-parse | 1 | 48.59 / 48.62 / 94.27 / 52.28 |
| records_array_8m / retain-parse | 4 | 89.34 / 89.30 / 146.88 / 91.91 |
| records_array_8m / retain-stringify | 1 | 75.72 / 75.75 / 114.12 / 73.44 |
| records_array_8m / retain-stringify | 4 | 95.02 / 95.05 / 143.81 / 94.42 |
| records_object_8m / retain-parse | 1 | 42.98 / 43.02 / 94.28 / 52.27 |
| records_object_8m / retain-parse | 4 | 113.42 / 113.47 / 147.03 / 91.80 |
| records_object_8m / retain-stringify | 1 | 75.83 / 75.86 / 114.14 / 73.42 |
| records_object_8m / retain-stringify | 4 | 95.12 / 95.16 / 143.64 / 94.44 |
| long_string_1m / retain-parse | 1 | 15.38 / 15.41 / 56.50 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.98 / 56.94 / 89.08 / 61.62 |
| long_string_1m / retain-stringify | 1 | 21.09 / 21.12 / 60.52 / 32.59 |
| long_string_1m / retain-stringify | 32 | 59.91 / 59.86 / 92.28 / 63.67 |
| unicode_1m / retain-parse | 1 | 14.66 / 14.58 / 57.61 / 31.23 |
| unicode_1m / retain-parse | 32 | 44.73 / 44.77 / 86.33 / 58.94 |
| unicode_1m / retain-stringify | 1 | 18.66 / 18.67 / 60.86 / 34.03 |
| unicode_1m / retain-stringify | 32 | 48.59 / 48.62 / 89.12 / 61.75 |
| wide_1m / retain-parse | 1 | 24.55 / 24.56 / 65.53 / 36.27 |
| wide_1m / retain-parse | 16 | 75.62 / 75.66 / 112.44 / 68.88 |
| wide_1m / retain-stringify | 1 | 30.66 / 30.70 / 68.64 / 39.20 |
| wide_1m / retain-stringify | 16 | 45.92 / 45.97 / 86.42 / 53.44 |

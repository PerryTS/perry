Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.014 / 0.014 / 0.027 / 0.019 | 1.02× | 13.47 / 13.38 / 57.61 / 35.67 |
| null / stringify | 2,000,000 | 0.007 / 0.007 / 0.027 / 0.029 | 1.00× | 13.56 / 13.50 / 59.52 / 129.75 |
| string_a / parse | 2,000,000 | 0.017 / 0.018 / 0.032 / 0.023 | 0.95× | 13.47 / 13.38 / 57.53 / 36.05 |
| string_a / stringify | 2,000,000 | 0.010 / 0.010 / 0.030 / 0.029 | 1.00× | 13.55 / 13.50 / 59.56 / 129.80 |
| empty_object / parse | 2,000,000 | 0.326 / 0.322 / 0.045 / 0.024 | 1.01× | 34.47 / 34.36 / 59.50 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.050 / 0.050 / 0.032 / 0.030 | 1.01× | 13.67 / 13.64 / 59.56 / 129.77 |
| tiny_object / parse | 2,000,000 | 0.360 / 0.357 / 0.081 / 0.045 | 1.01× | 34.45 / 34.39 / 59.45 / 68.98 |
| tiny_object / stringify | 2,000,000 | 0.095 / 0.095 / 0.037 / 0.039 | 1.00× | 34.67 / 34.61 / 59.48 / 129.73 |
| small_record / parse | 506,614 | 0.686 / 0.676 / 0.349 / 0.255 | 1.01× | 34.84 / 34.77 / 59.55 / 79.81 |
| small_record / stringify | 1,265,374 | 0.315 / 0.315 / 0.109 / 0.121 | 1.00× | 35.81 / 35.77 / 59.61 / 253.16 |
| object_1k / parse | 559,918 | 0.611 / 0.595 / 0.540 / 0.237 | 1.03× | 34.91 / 34.83 / 61.64 / 71.12 |
| object_1k / stringify | 637,619 | 0.246 / 0.245 / 0.199 / 0.215 | 1.00× | 35.80 / 35.75 / 61.72 / 70.80 |
| records_array_16k / parse | 6,660 | 23.885 / 23.804 / 39.162 / 33.944 | 1.00× | 64.92 / 64.78 / 65.75 / 70.41 |
| records_array_16k / stringify | 10,014 | 24.419 / 24.206 / 13.269 / 23.252 | 1.01× | 36.08 / 36.05 / 61.84 / 71.00 |
| records_array_1m / parse | 94 | 1,519.883 / 1,550.309 / 2,681.043 / 2,135.660 | 0.98× | 62.19 / 61.97 / 97.97 / 79.09 |
| records_array_1m / stringify | 177 | 1,553.588 / 1,551.757 / 849.384 / 970.514 | 1.00× | 60.59 / 60.53 / 109.11 / 100.31 |
| records_object_1m / parse | 70 | 4,036.657 / 4,032.486 / 2,671.929 / 2,137.286 | 1.00× | 79.98 / 79.89 / 92.73 / 75.94 |
| records_object_1m / stringify | 175 | 1,555.234 / 1,552.349 / 845.566 / 973.874 | 1.00× | 60.39 / 60.33 / 107.47 / 100.31 |
| records_array_8m / parse | 10 | 13,634.300 / 13,376.500 / 34,204.900 / 20,810.200 | 1.02× | 102.64 / 99.38 / 243.98 / 131.88 |
| records_array_8m / stringify | 20 | 13,533.650 / 13,495.350 / 6,831.500 / 8,451.350 | 1.00× | 195.83 / 195.78 / 186.12 / 169.59 |
| records_object_8m / parse | 8 | 31,716.625 / 31,389.250 / 29,607.625 / 21,748.750 | 1.01× | 231.69 / 231.61 / 225.42 / 111.77 |
| records_object_8m / stringify | 20 | 13,521.300 / 13,456.950 / 6,811.800 / 8,451.400 | 1.00× | 195.91 / 195.86 / 185.95 / 169.62 |
| records_array_20m / parse | 3 | 78,625.667 / 77,287.333 / 99,126.333 / 56,735.333 | 1.02× | 315.72 / 315.61 / 326.02 / 187.53 |
| records_array_20m / stringify | 8 | 86,415.500 / 86,059.500 / 17,342.375 / 21,028.875 | 1.00× | 171.23 / 171.12 / 392.09 / 304.89 |
| records_object_20m / parse | 3 | 78,210.000 / 77,381.333 / 97,830.667 / 56,661.333 | 1.01× | 315.70 / 315.59 / 325.75 / 187.70 |
| records_object_20m / stringify | 8 | 86,056.250 / 85,879.750 / 17,300.250 / 20,830.625 | 1.00× | 171.23 / 171.12 / 392.20 / 304.78 |
| numbers_1m / parse | 91 | 1,721.681 / 1,717.692 / 3,125.879 / 3,190.527 | 1.00× | 64.47 / 64.80 / 86.30 / 67.19 |
| numbers_1m / stringify | 95 | 1,645.547 / 1,606.347 / 1,983.779 / 2,948.905 | 1.02× | 61.30 / 61.19 / 91.30 / 72.22 |
| long_string_1m / parse | 1,292 | 215.230 / 206.211 / 368.885 / 67.686 | 1.04× | 60.80 / 60.70 / 158.86 / 158.41 |
| long_string_1m / stringify | 1,169 | 173.336 / 172.653 / 105.604 / 96.992 | 1.00× | 61.95 / 61.84 / 158.94 / 151.58 |
| escaped_1m / parse | 82 | 2,039.646 / 2,047.659 / 1,733.610 / 2,086.939 | 1.00× | 58.17 / 58.06 / 73.94 / 64.17 |
| escaped_1m / stringify | 83 | 1,863.940 / 1,861.313 / 1,867.699 / 2,089.410 | 1.00× | 56.97 / 56.86 / 79.56 / 68.98 |
| unicode_1m / parse | 1,537 | 266.072 / 256.371 / 437.664 / 60.541 | 1.04× | 55.50 / 55.39 / 166.75 / 165.09 |
| unicode_1m / stringify | 1,416 | 138.665 / 138.934 / 412.581 / 451.754 | 1.00× | 57.17 / 57.09 / 157.56 / 125.83 |
| wide_1m / parse | 36 | 20,909.639 / 20,868.861 / 4,962.444 / 4,162.750 | 1.00× | 176.64 / 176.52 / 110.50 / 86.14 |
| wide_1m / stringify | 216 | 1,707.569 / 1,690.870 / 6,358.255 / 666.417 | 1.01× | 70.89 / 70.78 / 116.39 / 83.27 |
| heterogeneous_1m / parse | 80 | 1,913.213 / 1,936.000 / 3,849.537 / 2,973.350 | 0.99× | 67.97 / 69.19 / 92.14 / 80.80 |
| heterogeneous_1m / stringify | 167 | 4,400.251 / 4,430.862 / 906.395 / 1,060.078 | 0.99× | 59.12 / 59.06 / 110.39 / 100.52 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.17 / 12.11 / 52.61 / 28.30 |
| tiny_object / retain-parse | 200,000 | 24.77 / 24.72 / 74.77 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.28 / 12.25 / 52.73 / 28.27 |
| tiny_object / retain-stringify | 200,000 | 24.86 / 24.84 / 71.22 / 47.02 |
| small_record / retain-parse | 1 | 12.19 / 12.12 / 52.64 / 28.25 |
| small_record / retain-parse | 100,000 | 56.80 / 56.75 / 85.48 / 52.67 |
| small_record / retain-stringify | 1 | 12.33 / 12.31 / 52.78 / 28.36 |
| small_record / retain-stringify | 100,000 | 28.06 / 28.06 / 80.30 / 51.05 |
| records_array_1m / retain-parse | 1 | 17.83 / 16.30 / 58.45 / 31.61 |
| records_array_1m / retain-parse | 16 | 41.25 / 41.48 / 88.59 / 55.88 |
| records_array_1m / retain-stringify | 1 | 19.97 / 19.95 / 60.58 / 34.39 |
| records_array_1m / retain-stringify | 16 | 33.86 / 33.84 / 76.30 / 47.22 |
| records_object_1m / retain-parse | 1 | 15.84 / 15.80 / 58.36 / 31.66 |
| records_object_1m / retain-parse | 16 | 66.88 / 66.88 / 88.73 / 55.89 |
| records_object_1m / retain-stringify | 1 | 19.97 / 19.97 / 60.48 / 34.41 |
| records_object_1m / retain-stringify | 16 | 33.88 / 33.88 / 76.30 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.50 / 47.97 / 94.28 / 52.31 |
| records_array_8m / retain-parse | 4 | 89.25 / 86.36 / 146.88 / 91.81 |
| records_array_8m / retain-stringify | 1 | 75.45 / 75.44 / 114.20 / 73.42 |
| records_array_8m / retain-stringify | 4 | 94.75 / 94.75 / 143.66 / 94.45 |
| records_object_8m / retain-parse | 1 | 42.81 / 42.81 / 94.30 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.23 / 113.20 / 146.95 / 91.80 |
| records_object_8m / retain-stringify | 1 | 75.53 / 75.53 / 114.14 / 73.45 |
| records_object_8m / retain-stringify | 4 | 94.83 / 94.83 / 143.86 / 94.45 |
| long_string_1m / retain-parse | 1 | 15.30 / 15.25 / 56.52 / 30.55 |
| long_string_1m / retain-parse | 32 | 56.88 / 56.77 / 89.11 / 61.62 |
| long_string_1m / retain-stringify | 1 | 18.73 / 18.73 / 60.52 / 32.61 |
| long_string_1m / retain-stringify | 32 | 58.78 / 58.69 / 92.28 / 63.72 |
| unicode_1m / retain-parse | 1 | 14.48 / 14.44 / 57.61 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.70 / 44.66 / 86.33 / 58.94 |
| unicode_1m / retain-stringify | 1 | 16.70 / 16.69 / 60.86 / 34.47 |
| unicode_1m / retain-stringify | 32 | 46.66 / 46.66 / 89.11 / 61.77 |
| wide_1m / retain-parse | 1 | 24.03 / 24.00 / 65.56 / 36.28 |
| wide_1m / retain-parse | 16 | 71.03 / 71.19 / 112.47 / 68.84 |
| wide_1m / retain-stringify | 1 | 30.12 / 30.08 / 68.64 / 39.22 |
| wide_1m / retain-stringify | 16 | 45.39 / 45.34 / 86.45 / 53.42 |

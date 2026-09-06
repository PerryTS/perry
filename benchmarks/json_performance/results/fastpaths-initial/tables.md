Medians of three fresh processes; CPU includes user + system time.

Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.
Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;
stringify starts with a fully materialized value. RSS is whole-process memory.

| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |
|---|---:|---:|---:|---:|
| null / parse | 2,000,000 | 0.249 / 0.255 / 0.027 / 0.019 | 0.97× | 13.62 / 13.66 / 57.55 / 35.67 |
| null / stringify | 2,000,000 | 0.066 / 0.064 / 0.027 / 0.029 | 1.03× | 34.80 / 34.78 / 59.52 / 129.75 |
| string_a / parse | 2,000,000 | 0.253 / 0.257 / 0.032 / 0.023 | 0.98× | 13.62 / 13.66 / 57.58 / 36.06 |
| string_a / stringify | 2,000,000 | 0.073 / 0.072 / 0.030 / 0.029 | 1.02× | 34.77 / 34.73 / 59.48 / 129.75 |
| empty_object / parse | 2,000,000 | 0.324 / 0.329 / 0.045 / 0.024 | 0.98× | 34.58 / 34.55 / 59.47 / 69.02 |
| empty_object / stringify | 2,000,000 | 0.137 / 0.134 / 0.032 / 0.030 | 1.02× | 34.84 / 34.81 / 59.55 / 129.73 |
| tiny_object / parse | 2,000,000 | 0.367 / 0.371 / 0.081 / 0.046 | 0.99× | 34.55 / 34.55 / 59.50 / 69.02 |
| tiny_object / stringify | 2,000,000 | 0.196 / 0.194 / 0.037 / 0.040 | 1.01× | 34.86 / 34.83 / 59.58 / 129.73 |
| small_record / parse | 590,213 | 0.807 / 0.767 / 0.351 / 0.253 | 1.05× | 34.94 / 34.95 / 59.58 / 79.88 |
| small_record / stringify | 1,278,412 | 0.562 / 0.548 / 0.109 / 0.120 | 1.03× | 35.91 / 35.92 / 59.64 / 255.39 |
| object_1k / parse | 557,534 | 1.760 / 0.600 / 0.545 / 0.237 | 2.93× | 34.97 / 34.95 / 61.72 / 71.11 |
| object_1k / stringify | 642,512 | 1.135 / 0.386 / 0.199 / 0.215 | 2.94× | 36.02 / 35.97 / 61.69 / 70.88 |
| records_array_16k / parse | 6,190 | 30.134 / 25.189 / 40.396 / 34.002 | 1.20× | 64.91 / 64.94 / 65.77 / 70.42 |
| records_array_16k / stringify | 9,972 | 37.561 / 35.879 / 13.205 / 23.184 | 1.05× | 36.25 / 36.23 / 61.75 / 70.98 |
| records_array_1m / parse | 86 | 1,976.512 / 1,654.244 / 2,929.802 / 2,145.477 | 1.19× | 62.19 / 62.20 / 92.67 / 79.14 |
| records_array_1m / stringify | 175 | 2,360.920 / 2,235.520 / 848.069 / 969.634 | 1.06× | 60.64 / 61.53 / 107.42 / 100.25 |
| records_object_1m / parse | 71 | 4,558.704 / 4,112.197 / 2,902.479 / 2,132.014 | 1.11× | 80.34 / 80.36 / 92.67 / 75.86 |
| records_object_1m / stringify | 175 | 2,370.897 / 2,241.149 / 849.194 / 969.474 | 1.06× | 60.66 / 61.53 / 107.42 / 100.27 |
| records_array_8m / parse | 9 | 17,119.556 / 14,598.333 / 34,755.222 / 21,121.778 | 1.17× | 102.66 / 102.69 / 243.61 / 118.58 |
| records_array_8m / stringify | 20 | 19,876.050 / 18,928.100 / 6,803.700 / 8,514.550 | 1.05× | 195.95 / 195.92 / 186.06 / 189.88 |
| records_object_8m / parse | 8 | 35,219.625 / 32,242.500 / 29,771.875 / 21,548.750 | 1.09× | 231.72 / 231.67 / 225.48 / 110.88 |
| records_object_8m / stringify | 20 | 19,881.400 / 18,882.300 / 6,795.650 / 8,452.800 | 1.05× | 195.97 / 195.92 / 186.12 / 169.77 |
| records_array_20m / parse | 3 | 87,346.667 / 80,050.000 / 98,021.667 / 56,723.333 | 1.09× | 315.69 / 315.66 / 325.84 / 187.86 |
| records_array_20m / stringify | 8 | 101,816.500 / 97,868.750 / 17,459.000 / 20,751.625 | 1.04× | 176.11 / 176.09 / 392.08 / 320.84 |
| records_object_20m / parse | 3 | 87,546.667 / 80,291.667 / 98,173.000 / 56,397.000 | 1.09× | 315.81 / 315.75 / 326.22 / 188.03 |
| records_object_20m / stringify | 8 | 101,505.500 / 97,847.125 / 17,398.375 / 20,859.250 | 1.04× | 171.33 / 171.25 / 392.11 / 304.89 |
| numbers_1m / parse | 66 | 2,261.091 / 2,426.303 / 3,070.439 / 3,196.955 | 0.93× | 64.73 / 64.47 / 80.02 / 65.16 |
| numbers_1m / stringify | 75 | 7,907.893 / 7,905.987 / 1,931.733 / 2,952.600 | 1.00× | 60.34 / 60.39 / 81.78 / 70.22 |
| long_string_1m / parse | 1,062 | 1,440.672 / 206.564 / 370.468 / 68.536 | 6.97× | 60.80 / 62.86 / 151.83 / 153.36 |
| long_string_1m / stringify | 1,110 | 978.086 / 203.117 / 105.450 / 95.953 | 4.82× | 63.95 / 64.97 / 156.84 / 123.52 |
| escaped_1m / parse | 82 | 2,611.573 / 2,040.085 / 1,733.024 / 2,084.793 | 1.28× | 58.20 / 58.23 / 73.86 / 64.19 |
| escaped_1m / stringify | 83 | 1,904.229 / 1,859.651 / 1,850.361 / 2,091.349 | 1.02× | 57.02 / 56.97 / 79.58 / 69.06 |
| unicode_1m / parse | 749 | 2,041.618 / 983.874 / 445.147 / 60.546 | 2.08× | 55.45 / 56.36 / 146.62 / 98.11 |
| unicode_1m / stringify | 329 | 1,640.492 / 984.213 / 434.304 / 456.307 | 1.67× | 58.22 / 59.06 / 129.31 / 81.39 |
| wide_1m / parse | 3 | 405,563.333 / 10,832.667 / 5,628.667 / 4,110.333 | 37.44× | 38.52 / 41.33 / 79.86 / 45.86 |
| wide_1m / stringify | 216 | 2,770.088 / 2,526.537 / 6,338.819 / 665.106 | 1.10× | 71.03 / 70.69 / 116.31 / 84.50 |
| heterogeneous_1m / parse | 72 | 2,148.972 / 2,014.931 / 3,883.944 / 2,959.972 | 1.07× | 67.97 / 67.98 / 92.25 / 77.33 |
| heterogeneous_1m / stringify | 168 | 4,445.607 / 4,496.833 / 904.804 / 1,059.333 | 0.99× | 59.17 / 60.00 / 110.52 / 100.53 |

Retained outputs (current RSS after the loop; all results remain live):

| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |
|---|---:|---:|
| tiny_object / retain-parse | 1 | 12.34 / 12.44 / 52.56 / 28.25 |
| tiny_object / retain-parse | 200,000 | 24.91 / 25.00 / 74.75 / 50.20 |
| tiny_object / retain-stringify | 1 | 12.56 / 12.64 / 52.75 / 28.31 |
| tiny_object / retain-stringify | 200,000 | 36.34 / 36.33 / 71.30 / 47.02 |
| small_record / retain-parse | 1 | 12.38 / 12.48 / 52.72 / 28.27 |
| small_record / retain-parse | 100,000 | 56.94 / 56.97 / 85.44 / 52.67 |
| small_record / retain-stringify | 1 | 12.62 / 12.72 / 52.73 / 28.28 |
| small_record / retain-stringify | 100,000 | 31.44 / 31.52 / 80.27 / 51.05 |
| records_array_1m / retain-parse | 1 | 18.09 / 18.09 / 58.42 / 31.61 |
| records_array_1m / retain-parse | 16 | 41.38 / 41.50 / 88.64 / 55.91 |
| records_array_1m / retain-stringify | 1 | 20.17 / 20.27 / 60.50 / 34.44 |
| records_array_1m / retain-stringify | 16 | 34.06 / 34.16 / 76.27 / 47.27 |
| records_object_1m / retain-parse | 1 | 16.03 / 16.12 / 58.31 / 31.62 |
| records_object_1m / retain-parse | 16 | 66.98 / 67.05 / 88.75 / 55.88 |
| records_object_1m / retain-stringify | 1 | 20.17 / 20.28 / 60.58 / 34.42 |
| records_object_1m / retain-stringify | 16 | 34.08 / 34.19 / 76.34 / 47.22 |
| records_array_8m / retain-parse | 1 | 48.66 / 48.77 / 94.22 / 52.31 |
| records_array_8m / retain-parse | 4 | 89.22 / 89.30 / 146.86 / 91.81 |
| records_array_8m / retain-stringify | 1 | 75.64 / 75.67 / 114.19 / 73.45 |
| records_array_8m / retain-stringify | 4 | 94.94 / 94.97 / 143.92 / 94.47 |
| records_object_8m / retain-parse | 1 | 43.08 / 43.19 / 94.34 / 52.30 |
| records_object_8m / retain-parse | 4 | 113.41 / 113.38 / 146.88 / 91.80 |
| records_object_8m / retain-stringify | 1 | 75.77 / 75.80 / 114.14 / 73.44 |
| records_object_8m / retain-stringify | 4 | 95.06 / 95.09 / 143.98 / 94.47 |
| long_string_1m / retain-parse | 1 | 15.44 / 15.55 / 56.50 / 30.53 |
| long_string_1m / retain-parse | 32 | 56.84 / 56.92 / 89.11 / 61.64 |
| long_string_1m / retain-stringify | 1 | 21.14 / 21.23 / 60.56 / 32.62 |
| long_string_1m / retain-stringify | 32 | 59.78 / 59.81 / 92.22 / 63.72 |
| unicode_1m / retain-parse | 1 | 14.59 / 14.70 / 57.64 / 31.25 |
| unicode_1m / retain-parse | 32 | 44.80 / 44.91 / 86.27 / 58.94 |
| unicode_1m / retain-stringify | 1 | 18.66 / 18.78 / 60.83 / 34.05 |
| unicode_1m / retain-stringify | 32 | 48.62 / 48.72 / 89.14 / 61.77 |
| wide_1m / retain-parse | 1 | 22.86 / 24.72 / 65.53 / 36.31 |
| wide_1m / retain-parse | 16 | 73.61 / 75.80 / 112.45 / 68.86 |
| wide_1m / retain-stringify | 1 | 29.45 / 30.77 / 68.53 / 39.22 |
| wide_1m / retain-stringify | 16 | 44.75 / 46.05 / 86.44 / 53.45 |

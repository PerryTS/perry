# All 38 parse and stringify rows

Apple M1, 8 GiB, macOS 26.5.1; Node 26.5.1 and Bun 1.3.14. Five fresh processes per engine and row, fixed identical iteration counts. CPU is user + system time per operation; memory is whole-process peak RSS. Default GC is enabled.

The previous Perry build is depth22 (`ae9283ca3`); the structural scan is depth24 (`15d2aeafd`). The GC reference is `0327b9460`. All Perry builds use the same managed argv[0], CWD and fixture paths via the qualified immutable execv launcher. Input and output correctness are checked separately.

Perry may defer materialization when parsing array roots in the automatic tape size window; a parse-only result is not a measurement of consuming every element. Stringify inputs are fully materialized. Retained-memory and full-lifetime measurements are saved alongside these tables.

## CPU, microseconds per call

| Fixture | Operation | GC reference | Previous Perry | Structural Perry | Node | Bun | Change vs previous |
|---|---|---:|---:|---:|---:|---:|---:|
| empty_object | parse | 0.056 | 0.056 | 0.056 | 0.045 | 0.024 | -0.00% |
| empty_object | stringify | 0.048 | 0.048 | 0.048 | 0.032 | 0.030 | -0.01% |
| escaped_1m | parse | 2454.378 | 1151.037 | 1136.610 | 1732.854 | 2084.244 | -1.25% |
| escaped_1m | stringify | 963.667 | 963.065 | 963.268 | 1892.875 | 2084.244 | +0.02% |
| heterogeneous_1m | parse | 1856.400 | 1847.575 | 1416.200 | 3904.988 | 2949.463 | -23.35% |
| heterogeneous_1m | stringify | 796.483 | 798.066 | 794.815 | 905.081 | 1055.910 | -0.41% |
| long_string_1m | parse | 201.135 | 191.732 | 192.674 | 368.894 | 68.191 | +0.49% |
| long_string_1m | stringify | 170.683 | 171.052 | 170.744 | 106.128 | 96.405 | -0.18% |
| null | parse | 0.012 | 0.012 | 0.012 | 0.027 | 0.019 | +2.64% |
| null | stringify | 0.007 | 0.007 | 0.007 | 0.027 | 0.029 | +0.02% |
| numbers_1m | parse | 1704.467 | 1703.728 | 1701.859 | 3126.641 | 3203.370 | -0.11% |
| numbers_1m | stringify | 1544.062 | 1577.719 | 1575.979 | 1979.406 | 2949.302 | -0.11% |
| object_1k | parse | 0.517 | 0.512 | 0.508 | 0.542 | 0.237 | -0.89% |
| object_1k | stringify | 0.238 | 0.237 | 0.238 | 0.199 | 0.215 | +0.22% |
| records_array_16k | parse | 22.532 | 22.539 | 18.268 | 40.126 | 33.995 | -18.95% |
| records_array_16k | stringify | 22.764 | 22.754 | 22.755 | 13.212 | 23.224 | +0.00% |
| records_array_1m | parse | 1473.653 | 1477.547 | 1200.432 | 2724.389 | 2136.411 | -18.76% |
| records_array_1m | stringify | 1459.266 | 1459.243 | 1458.548 | 849.898 | 968.689 | -0.05% |
| records_array_20m | parse | 55074.333 | 55302.333 | 50167.333 | 95240.333 | 56804.333 | -9.29% |
| records_array_20m | stringify | 78026.250 | 78030.500 | 78057.750 | 17428.625 | 20804.625 | +0.03% |
| records_array_8m | parse | 12779.900 | 12772.500 | 10612.700 | 34300.000 | 20879.600 | -16.91% |
| records_array_8m | stringify | 12642.000 | 12660.350 | 12671.100 | 6811.850 | 8490.950 | +0.08% |
| records_object_1m | parse | 2886.803 | 2909.085 | 2636.437 | 2738.493 | 2132.577 | -9.37% |
| records_object_1m | stringify | 1455.230 | 1460.276 | 1458.086 | 847.649 | 972.063 | -0.15% |
| records_object_20m | parse | 54923.667 | 55197.333 | 50077.333 | 97702.000 | 56808.000 | -9.28% |
| records_object_20m | stringify | 78130.125 | 78124.375 | 78106.750 | 17458.375 | 20897.250 | -0.02% |
| records_object_8m | parse | 22162.500 | 22265.250 | 20121.000 | 30077.625 | 21739.500 | -9.63% |
| records_object_8m | stringify | 12566.286 | 12579.524 | 12601.476 | 6788.714 | 8405.190 | +0.17% |
| small_record | parse | 0.523 | 0.527 | 0.530 | 0.354 | 0.253 | +0.53% |
| small_record | stringify | 0.284 | 0.283 | 0.283 | 0.109 | 0.121 | +0.22% |
| string_a | parse | 0.015 | 0.014 | 0.015 | 0.032 | 0.023 | +2.15% |
| string_a | stringify | 0.010 | 0.010 | 0.010 | 0.030 | 0.029 | +0.00% |
| tiny_object | parse | 0.123 | 0.123 | 0.123 | 0.081 | 0.046 | -0.05% |
| tiny_object | stringify | 0.094 | 0.094 | 0.094 | 0.037 | 0.039 | +0.04% |
| unicode_1m | parse | 254.993 | 213.128 | 213.127 | 437.796 | 60.093 | -0.00% |
| unicode_1m | stringify | 135.321 | 135.294 | 135.436 | 414.275 | 450.133 | +0.10% |
| wide_1m | parse | 15151.056 | 15158.611 | 14909.778 | 4958.667 | 4161.278 | -1.64% |
| wide_1m | stringify | 1672.735 | 1671.874 | 1669.851 | 6339.433 | 666.102 | -0.12% |

## Peak RSS, MiB

RSS includes process/runtime overhead; it is not the size of the JSON output or GC heap alone.

| Fixture | Operation | GC reference | Previous Perry | Structural Perry | Node | Bun | Change vs previous, KiB |
|---|---|---:|---:|---:|---:|---:|---:|
| empty_object | parse | 33.594 | 33.609 | 33.656 | 59.469 | 69.016 | +48 |
| empty_object | stringify | 12.828 | 12.859 | 12.875 | 59.500 | 129.766 | +16 |
| escaped_1m | parse | 57.734 | 57.047 | 57.859 | 74.062 | 64.203 | +832 |
| escaped_1m | stringify | 56.000 | 56.016 | 56.125 | 124.672 | 74.766 | +112 |
| heterogeneous_1m | parse | 68.219 | 68.250 | 68.328 | 92.297 | 80.828 | +80 |
| heterogeneous_1m | stringify | 57.922 | 57.953 | 58.047 | 128.734 | 103.141 | +96 |
| long_string_1m | parse | 59.609 | 59.656 | 59.750 | 159.922 | 162.375 | +96 |
| long_string_1m | stringify | 60.766 | 60.812 | 60.891 | 156.922 | 123.547 | +80 |
| null | parse | 12.625 | 12.672 | 12.641 | 57.562 | 35.688 | -32 |
| null | stringify | 12.734 | 12.766 | 12.766 | 59.500 | 129.766 | +0 |
| numbers_1m | parse | 63.000 | 63.031 | 63.109 | 87.375 | 68.188 | +80 |
| numbers_1m | stringify | 62.438 | 62.281 | 62.578 | 92.391 | 72.219 | +304 |
| object_1k | parse | 33.781 | 33.844 | 33.844 | 61.672 | 71.125 | +0 |
| object_1k | stringify | 34.703 | 34.719 | 34.797 | 61.672 | 70.828 | +80 |
| records_array_16k | parse | 63.703 | 63.734 | 63.812 | 65.703 | 70.422 | +80 |
| records_array_16k | stringify | 34.953 | 34.922 | 35.000 | 61.750 | 71.000 | +80 |
| records_array_1m | parse | 60.938 | 60.953 | 61.047 | 92.891 | 79.125 | +96 |
| records_array_1m | stringify | 58.141 | 58.125 | 58.250 | 109.328 | 100.312 | +128 |
| records_array_20m | parse | 223.094 | 223.078 | 223.156 | 326.250 | 187.922 | +80 |
| records_array_20m | stringify | 157.531 | 157.500 | 157.641 | 392.141 | 304.891 | +144 |
| records_array_8m | parse | 98.844 | 98.891 | 98.953 | 244.078 | 131.250 | +64 |
| records_array_8m | stringify | 192.469 | 192.516 | 192.562 | 186.125 | 169.875 | +48 |
| records_object_1m | parse | 57.516 | 57.484 | 57.578 | 92.875 | 75.875 | +96 |
| records_object_1m | stringify | 58.141 | 58.141 | 58.266 | 106.719 | 100.297 | +128 |
| records_object_20m | parse | 223.094 | 223.078 | 223.156 | 326.000 | 187.828 | +80 |
| records_object_20m | stringify | 157.547 | 157.531 | 157.656 | 392.109 | 304.875 | +128 |
| records_object_8m | parse | 165.625 | 165.609 | 165.688 | 225.500 | 111.438 | +80 |
| records_object_8m | stringify | 192.516 | 192.547 | 192.625 | 186.047 | 176.406 | +80 |
| small_record | parse | 33.328 | 33.344 | 33.406 | 59.562 | 79.859 | +64 |
| small_record | stringify | 34.688 | 34.703 | 34.766 | 59.656 | 246.844 | +64 |
| string_a | parse | 12.625 | 12.656 | 12.641 | 57.609 | 36.062 | -16 |
| string_a | stringify | 12.734 | 12.750 | 12.750 | 59.484 | 129.766 | +0 |
| tiny_object | parse | 33.641 | 33.641 | 33.719 | 59.453 | 69.047 | +80 |
| tiny_object | stringify | 33.609 | 33.594 | 33.688 | 59.547 | 129.781 | +96 |
| unicode_1m | parse | 55.344 | 55.359 | 55.438 | 166.797 | 151.688 | +80 |
| unicode_1m | stringify | 55.969 | 56.000 | 56.062 | 157.641 | 126.266 | +64 |
| wide_1m | parse | 166.750 | 168.484 | 168.562 | 110.609 | 86.094 | +80 |
| wide_1m | stringify | 69.672 | 69.734 | 69.828 | 116.422 | 84.609 | +96 |

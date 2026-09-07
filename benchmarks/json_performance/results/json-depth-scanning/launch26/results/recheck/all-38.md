# All 38 parse/stringify rows

Qualified immutable-file run; five fresh-process trials per engine and row. CPU is loop user + system time per call. RSS is whole-process peak, including initialization. Depth22 is selected; depth23 is the later-entry experiment. All Perry arguments and working directories are held fixed. Ranges and instructions are in summary.json.

## CPU (microseconds per call)

| Fixture | Operation | GC checkpoint | Previous decoder | Depth22 selected | Depth23 | Node | Bun | Selected vs previous |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| empty_object | parse | 0.056 | 0.056 | 0.056 | 0.056 | 0.045 | 0.024 | +0.00% |
| empty_object | stringify | 0.048 | 0.048 | 0.048 | 0.048 | 0.032 | 0.030 | +0.02% |
| escaped_1m | parse | 2454.415 | 1587.220 | 1148.098 | 1150.134 | 1734.280 | 2083.427 | -27.67% |
| escaped_1m | stringify | 962.940 | 964.161 | 964.530 | 962.589 | 1892.940 | 2084.833 | +0.04% |
| heterogeneous_1m | parse | 1854.162 | 1844.362 | 1845.175 | 1823.838 | 3906.750 | 2981.562 | +0.04% |
| heterogeneous_1m | stringify | 795.896 | 796.294 | 795.991 | 796.185 | 904.787 | 1054.848 | -0.04% |
| long_string_1m | parse | 200.424 | 200.425 | 193.105 | 192.026 | 368.787 | 67.733 | -3.65% |
| long_string_1m | stringify | 172.442 | 171.585 | 171.344 | 171.626 | 106.021 | 97.681 | -0.14% |
| null | parse | 0.012 | 0.012 | 0.012 | 0.012 | 0.027 | 0.019 | -0.03% |
| null | stringify | 0.007 | 0.007 | 0.007 | 0.007 | 0.027 | 0.029 | -0.09% |
| numbers_1m | parse | 1702.652 | 1703.370 | 1705.087 | 1706.630 | 3125.217 | 3198.793 | +0.10% |
| numbers_1m | stringify | 1544.156 | 1572.833 | 1577.917 | 1578.875 | 1982.312 | 2949.927 | +0.32% |
| object_1k | parse | 0.517 | 0.518 | 0.514 | 0.513 | 0.544 | 0.237 | -0.76% |
| object_1k | stringify | 0.238 | 0.239 | 0.237 | 0.238 | 0.199 | 0.215 | -0.48% |
| records_array_16k | parse | 22.532 | 22.448 | 22.545 | 22.196 | 39.512 | 33.950 | +0.43% |
| records_array_16k | stringify | 22.741 | 22.826 | 22.761 | 22.789 | 13.261 | 23.254 | -0.28% |
| records_array_1m | parse | 1473.989 | 1470.116 | 1478.789 | 1455.821 | 2855.242 | 2137.747 | +0.59% |
| records_array_1m | stringify | 1458.763 | 1462.373 | 1457.718 | 1460.537 | 848.910 | 971.028 | -0.32% |
| records_array_20m | parse | 54756.667 | 55074.333 | 54941.000 | 54907.667 | 97089.667 | 56809.333 | -0.24% |
| records_array_20m | stringify | 77985.500 | 78040.625 | 78035.500 | 78122.375 | 17430.375 | 20786.375 | -0.01% |
| records_array_8m | parse | 12796.000 | 12766.700 | 12750.400 | 12665.100 | 34710.200 | 21017.300 | -0.13% |
| records_array_8m | stringify | 12647.700 | 12675.650 | 12641.450 | 12651.150 | 6796.800 | 8403.400 | -0.27% |
| records_object_1m | parse | 2888.310 | 2906.268 | 2907.859 | 2889.986 | 2897.324 | 2139.972 | +0.05% |
| records_object_1m | stringify | 1454.753 | 1465.138 | 1459.011 | 1462.868 | 843.776 | 970.500 | -0.42% |
| records_object_20m | parse | 54792.667 | 55352.667 | 55265.333 | 55032.333 | 95862.667 | 56461.000 | -0.16% |
| records_object_20m | stringify | 78125.125 | 78203.375 | 78188.375 | 78100.250 | 17393.375 | 20857.625 | -0.02% |
| records_object_8m | parse | 22143.000 | 22305.250 | 22253.375 | 22197.500 | 29682.375 | 21747.500 | -0.23% |
| records_object_8m | stringify | 12559.095 | 12607.762 | 12591.190 | 12589.381 | 6780.524 | 8409.524 | -0.13% |
| small_record | parse | 0.523 | 0.527 | 0.527 | 0.528 | 0.359 | 0.253 | +0.01% |
| small_record | stringify | 0.285 | 0.284 | 0.283 | 0.284 | 0.108 | 0.121 | -0.25% |
| string_a | parse | 0.015 | 0.014 | 0.014 | 0.014 | 0.032 | 0.023 | +0.07% |
| string_a | stringify | 0.010 | 0.010 | 0.010 | 0.010 | 0.030 | 0.029 | +0.05% |
| tiny_object | parse | 0.124 | 0.124 | 0.123 | 0.123 | 0.081 | 0.045 | -0.27% |
| tiny_object | stringify | 0.094 | 0.094 | 0.094 | 0.094 | 0.037 | 0.039 | -0.25% |
| unicode_1m | parse | 255.316 | 221.012 | 213.090 | 212.362 | 438.120 | 60.453 | -3.58% |
| unicode_1m | stringify | 135.076 | 136.249 | 135.801 | 136.188 | 413.092 | 450.210 | -0.33% |
| wide_1m | parse | 15111.472 | 15102.417 | 15127.750 | 15125.806 | 4927.500 | 4163.111 | +0.17% |
| wide_1m | stringify | 1673.330 | 1671.819 | 1672.023 | 1673.163 | 6357.884 | 665.963 | +0.01% |

## Peak RSS (MiB)

| Fixture | Operation | Previous decoder | Depth22 selected | Depth23 | Node | Bun | Selected change (KiB) |
|---|---|---:|---:|---:|---:|---:|---:|
| empty_object | parse | 33.609 | 33.609 | 33.672 | 59.469 | 69.016 | +0 |
| empty_object | stringify | 12.812 | 12.859 | 12.891 | 59.516 | 129.766 | +48 |
| escaped_1m | parse | 57.750 | 57.047 | 57.125 | 74.094 | 64.172 | -720 |
| escaped_1m | stringify | 56.016 | 56.016 | 56.094 | 124.688 | 74.734 | +0 |
| heterogeneous_1m | parse | 68.219 | 68.250 | 68.328 | 92.266 | 80.812 | +32 |
| heterogeneous_1m | stringify | 57.922 | 57.953 | 58.031 | 128.891 | 102.297 | +32 |
| long_string_1m | parse | 59.625 | 59.672 | 59.734 | 159.938 | 156.359 | +48 |
| long_string_1m | stringify | 60.797 | 60.812 | 60.891 | 156.938 | 155.547 | +16 |
| null | parse | 12.594 | 12.672 | 12.688 | 57.625 | 35.703 | +80 |
| null | stringify | 12.703 | 12.750 | 12.781 | 59.484 | 129.781 | +48 |
| numbers_1m | parse | 63.000 | 63.031 | 63.109 | 87.406 | 68.188 | +32 |
| numbers_1m | stringify | 62.469 | 62.281 | 62.344 | 92.297 | 72.250 | -192 |
| object_1k | parse | 33.781 | 33.797 | 33.859 | 61.672 | 71.141 | +16 |
| object_1k | stringify | 34.719 | 34.719 | 34.781 | 61.672 | 70.828 | +0 |
| records_array_16k | parse | 63.703 | 63.734 | 63.812 | 65.750 | 70.453 | +32 |
| records_array_16k | stringify | 34.922 | 34.938 | 34.984 | 61.766 | 71.031 | +16 |
| records_array_1m | parse | 60.938 | 60.953 | 61.031 | 93.125 | 79.141 | +16 |
| records_array_1m | stringify | 58.125 | 58.125 | 58.203 | 109.219 | 100.297 | +0 |
| records_array_20m | parse | 223.094 | 223.078 | 223.141 | 326.062 | 188.016 | -16 |
| records_array_20m | stringify | 157.531 | 157.484 | 157.578 | 392.156 | 304.828 | -48 |
| records_array_8m | parse | 98.844 | 98.875 | 98.953 | 244.125 | 131.344 | +32 |
| records_array_8m | stringify | 192.469 | 192.516 | 192.594 | 186.094 | 189.703 | +48 |
| records_object_1m | parse | 57.516 | 57.484 | 57.547 | 92.875 | 75.922 | -32 |
| records_object_1m | stringify | 58.141 | 58.141 | 58.219 | 106.688 | 100.312 | +0 |
| records_object_20m | parse | 223.094 | 223.078 | 223.141 | 325.953 | 188.047 | -16 |
| records_object_20m | stringify | 157.562 | 157.531 | 157.594 | 392.109 | 304.891 | -32 |
| records_object_8m | parse | 165.625 | 165.609 | 165.672 | 225.531 | 111.453 | -16 |
| records_object_8m | stringify | 192.531 | 192.547 | 192.625 | 185.984 | 176.406 | +16 |
| small_record | parse | 33.328 | 33.344 | 33.406 | 59.578 | 79.859 | +16 |
| small_record | stringify | 34.703 | 34.703 | 34.781 | 59.625 | 246.844 | +0 |
| string_a | parse | 12.594 | 12.656 | 12.688 | 57.609 | 36.078 | +64 |
| string_a | stringify | 12.703 | 12.750 | 12.781 | 59.531 | 129.766 | +48 |
| tiny_object | parse | 33.656 | 33.641 | 33.703 | 59.500 | 69.062 | -16 |
| tiny_object | stringify | 33.609 | 33.594 | 33.672 | 59.547 | 129.781 | -16 |
| unicode_1m | parse | 55.328 | 55.359 | 55.438 | 166.953 | 157.969 | +32 |
| unicode_1m | stringify | 55.953 | 56.000 | 56.078 | 157.781 | 130.312 | +48 |
| wide_1m | parse | 168.484 | 168.500 | 168.562 | 110.594 | 86.125 | +16 |
| wide_1m | stringify | 69.703 | 69.734 | 69.812 | 116.531 | 83.344 | +32 |

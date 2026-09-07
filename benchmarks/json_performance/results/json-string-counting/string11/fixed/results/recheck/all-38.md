# All 38 rows with an identical executable path

Five fresh-process trials per arm and row. CPU includes user and system time per call; RSS is process peak. Observed ranges are not confidence intervals. This comparison corrects the invocation-path difference between the Perry arms. Node/Bun are output oracles in this window; their timing figures are in the separate four-engine run.

| Fixture | Operation | Reference µs | Candidate µs | CPU change | Ranges separated | Peak RSS change MiB |
|---|---|---:|---:|---:|---|---:|
| empty_object | parse | 0.056 | 0.057 | +1.026% | False | -0.0156 |
| empty_object | stringify | 0.049 | 0.049 | +0.102% | False | -0.0469 |
| escaped_1m | parse | 2458.183 | 2477.280 | +0.777% | False | -0.0156 |
| escaped_1m | stringify | 969.899 | 966.726 | -0.327% | False | -0.0156 |
| heterogeneous_1m | parse | 1863.287 | 1862.575 | -0.038% | False | -0.0156 |
| heterogeneous_1m | stringify | 799.820 | 800.673 | +0.107% | False | -0.0156 |
| long_string_1m | parse | 202.564 | 220.839 | +9.022% | False | +0.0000 |
| long_string_1m | stringify | 172.915 | 173.938 | +0.592% | False | +0.0000 |
| null | parse | 0.012 | 0.012 | -0.016% | False | -0.0469 |
| null | stringify | 0.007 | 0.007 | +0.091% | False | -0.0469 |
| numbers_1m | parse | 1711.065 | 1713.272 | +0.129% | False | -0.0156 |
| numbers_1m | stringify | 1554.344 | 1551.562 | -0.179% | False | -0.0156 |
| object_1k | parse | 0.520 | 0.518 | -0.219% | False | -0.0156 |
| object_1k | stringify | 0.240 | 0.239 | -0.284% | False | -0.0156 |
| records_array_16k | parse | 22.605 | 22.631 | +0.117% | False | -0.0156 |
| records_array_16k | stringify | 22.819 | 25.962 | +13.771% | False | -0.0156 |
| records_array_1m | parse | 1477.379 | 1480.611 | +0.219% | False | -0.0156 |
| records_array_1m | stringify | 1553.294 | 1461.508 | -5.909% | False | -0.0156 |
| records_array_20m | parse | 54799.333 | 54853.000 | +0.098% | False | -0.0156 |
| records_array_20m | stringify | 78078.625 | 78071.750 | -0.009% | False | -0.0156 |
| records_array_8m | parse | 12852.100 | 12857.300 | +0.040% | False | -0.0156 |
| records_array_8m | stringify | 12662.250 | 12644.450 | -0.141% | False | -0.0156 |
| records_object_1m | parse | 2895.479 | 2894.986 | -0.017% | False | -0.0156 |
| records_object_1m | stringify | 1563.420 | 1455.511 | -6.902% | False | -0.0156 |
| records_object_20m | parse | 54616.667 | 54632.000 | +0.028% | False | -0.0156 |
| records_object_20m | stringify | 78173.625 | 78136.250 | -0.048% | False | -0.0312 |
| records_object_8m | parse | 22155.875 | 22166.125 | +0.046% | False | -0.0156 |
| records_object_8m | stringify | 12575.238 | 12556.857 | -0.146% | False | -0.0156 |
| small_record | parse | 0.526 | 0.526 | +0.080% | False | -0.0156 |
| small_record | stringify | 0.307 | 0.286 | -6.793% | False | -0.0156 |
| string_a | parse | 0.015 | 0.015 | -0.156% | False | -0.0469 |
| string_a | stringify | 0.010 | 0.010 | +0.015% | False | -0.0312 |
| tiny_object | parse | 0.124 | 0.124 | +0.419% | False | -0.0156 |
| tiny_object | stringify | 0.095 | 0.094 | -0.234% | False | -0.0156 |
| unicode_1m | parse | 258.814 | 238.553 | -7.828% | True | -0.0469 |
| unicode_1m | stringify | 136.936 | 136.606 | -0.241% | False | -0.0469 |
| wide_1m | parse | 15269.417 | 15274.139 | +0.031% | False | -1.0000 |
| wide_1m | stringify | 1682.758 | 1711.451 | +1.705% | False | -0.0156 |

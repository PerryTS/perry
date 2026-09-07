# Scanner inlining control

Seven fresh processes per engine and row. Inline is decode17; outlined is decode18. Their only source difference is the inlining attribute on parse_string_bytes. CPU is user + system time per call; RSS is process peak. All Perry arms use the same invocation path. This is a focused comparison, not an all-row acceptance.

| Fixture | Operation | Previous µs | Inline µs | Outlined µs | Node µs | Bun µs | Outlined vs inline | Outlined RSS MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| empty_object | parse | 0.072 | 0.072 | 0.072 | 0.045 | 0.024 | -0.05% | 33.609 |
| escaped_1m | parse | 2747.646 | 1895.000 | 1891.073 | 1733.354 | 2084.122 | -0.21% | 57.016 |
| escaped_1m | stringify | 1102.310 | 1103.679 | 1112.393 | 1893.107 | 2083.613 | +0.79% | 55.984 |
| long_string_1m | parse | 223.196 | 221.534 | 222.099 | 369.117 | 67.343 | +0.26% | 59.625 |
| records_array_16k | stringify | 26.151 | 26.129 | 26.187 | 13.179 | 23.213 | +0.22% | 34.922 |
| records_object_1m | parse | 3189.254 | 3140.366 | 3208.803 | 2917.648 | 2136.972 | +2.18% | 57.484 |
| small_record | parse | 0.572 | 0.565 | 0.575 | 0.340 | 0.252 | +1.76% | 33.328 |
| small_record | stringify | 0.310 | 0.310 | 0.309 | 0.108 | 0.121 | -0.34% | 34.703 |
| unicode_1m | parse | 240.187 | 239.309 | 239.244 | 437.706 | 60.321 | -0.03% | 55.328 |

Latest record-output candidate: 53052fc09d47d956e8decf22f677a773f94d42cb.
Medians of three fresh processes on Apple M1 / 8 GiB. Node 26.5.1; Bun 1.3.14. CPU is user + system microseconds per call, with default GC.
Ratios are Perry time / competitor time: below 1 favors Perry. Array-root parsing may defer materialization; stringify inputs are fully materialized. Paired regression rechecks are complete; no-regression acceptance fails.

| # | Fixture | Operation | Perry µs | Node µs | Bun µs | Perry / Node | Perry / Bun |
|---:|---|---|---:|---:|---:|---:|---:|
| 1 | null | parse | 0.0138 | 0.0274 | 0.0191 | 0.50× | 0.72× |
| 2 | null | stringify | 0.0066 | 0.0273 | 0.0287 | 0.24× | 0.23× |
| 3 | string_a | parse | 0.0163 | 0.0318 | 0.0226 | 0.51× | 0.72× |
| 4 | string_a | stringify | 0.0100 | 0.0301 | 0.0295 | 0.33× | 0.34× |
| 5 | empty_object | parse | 0.3143 | 0.0452 | 0.0242 | 6.95× | 12.99× |
| 6 | empty_object | stringify | 0.0502 | 0.0321 | 0.0299 | 1.56× | 1.68× |
| 7 | tiny_object | parse | 0.3499 | 0.0813 | 0.0452 | 4.31× | 7.74× |
| 8 | tiny_object | stringify | 0.1365 | 0.0373 | 0.0396 | 3.66× | 3.45× |
| 9 | small_record | parse | 0.6694 | 0.3424 | 0.2515 | 1.95× | 2.66× |
| 10 | small_record | stringify | 0.4022 | 0.1083 | 0.1205 | 3.71× | 3.34× |
| 11 | object_1k | parse | 0.5860 | 0.5424 | 0.2375 | 1.08× | 2.47× |
| 12 | object_1k | stringify | 0.2867 | 0.2011 | 0.2147 | 1.43× | 1.34× |
| 13 | records_array_16k | parse | 24.031 | 38.600 | 33.970 | 0.62× | 0.71× |
| 14 | records_array_16k | stringify | 24.252 | 13.172 | 23.261 | 1.84× | 1.04× |
| 15 | records_array_1m | parse | 1,530.809 | 2,752.202 | 2,134.819 | 0.56× | 0.72× |
| 16 | records_array_1m | stringify | 1,558.739 | 846.932 | 970.551 | 1.84× | 1.61× |
| 17 | records_object_1m | parse | 4,019.437 | 2,740.915 | 2,138.662 | 1.47× | 1.88× |
| 18 | records_object_1m | stringify | 1,566.703 | 847.280 | 971.446 | 1.85× | 1.61× |
| 19 | records_array_8m | parse | 13,748.900 | 34,636.900 | 20,831.800 | 0.40× | 0.66× |
| 20 | records_array_8m | stringify | 13,549.750 | 6,783.600 | 8,454.200 | 2.00× | 1.60× |
| 21 | records_object_8m | parse | 31,491.625 | 30,114.500 | 21,716.750 | 1.05× | 1.45× |
| 22 | records_object_8m | stringify | 13,581.200 | 6,833.300 | 8,489.800 | 1.99× | 1.60× |
| 23 | records_array_20m | parse | 77,826.667 | 97,782.000 | 56,832.667 | 0.80× | 1.37× |
| 24 | records_array_20m | stringify | 86,501.875 | 17,354.000 | 21,003.875 | 4.98× | 4.12× |
| 25 | records_object_20m | parse | 77,804.667 | 94,889.333 | 56,547.000 | 0.82× | 1.38× |
| 26 | records_object_20m | stringify | 86,022.875 | 17,364.500 | 20,778.875 | 4.95× | 4.14× |
| 27 | numbers_1m | parse | 1,727.750 | 3,129.739 | 3,200.250 | 0.55× | 0.54× |
| 28 | numbers_1m | stringify | 1,644.474 | 1,981.768 | 2,951.063 | 0.83× | 0.56× |
| 29 | long_string_1m | parse | 204.825 | 368.796 | 67.643 | 0.56× | 3.03× |
| 30 | long_string_1m | stringify | 173.995 | 105.844 | 97.479 | 1.64× | 1.78× |
| 31 | escaped_1m | parse | 2,038.518 | 1,733.120 | 2,084.217 | 1.18× | 0.98× |
| 32 | escaped_1m | stringify | 1,906.568 | 1,853.086 | 2,094.000 | 1.03× | 0.91× |
| 33 | unicode_1m | parse | 257.178 | 437.642 | 60.096 | 0.59× | 4.28× |
| 34 | unicode_1m | stringify | 138.532 | 413.397 | 450.198 | 0.34× | 0.31× |
| 35 | wide_1m | parse | 20,963.083 | 4,964.500 | 4,154.750 | 4.22× | 5.05× |
| 36 | wide_1m | stringify | 1,706.921 | 6,355.875 | 665.704 | 0.27× | 2.56× |
| 37 | heterogeneous_1m | parse | 1,960.714 | 3,855.377 | 2,984.675 | 0.51× | 0.66× |
| 38 | heterogeneous_1m | stringify | 4,379.869 | 903.190 | 1,064.940 | 4.85× | 4.11× |

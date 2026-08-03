# Polyglot Compute-Microbench Results (auto-generated)

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.5.0 arm64 on MacBookPro · **Date:** 2026-08-03
**Perry version:** v0.5.1279

Headline = median wall-clock ms. Lower is better.

| Benchmark           | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun | Hermes |  Python |
|---------------------|-------|-------|-------|-------|-------|-------|-------|-------|--------|---------|
| fibonacci           |   394 |   306 |   298 |   435 |   387 |   273 |   924 |   512 |      - |   12140 |
| loop_overhead       |    93 |    93 |    93 |    93 |    93 |    94 |    63 |    39 |      - |    1941 |
| loop_data_dependent |   218 |   218 |   124 |   124 |   217 |   220 |   220 |   220 |      - |    5947 |
| array_write         |     2 |     6 |     1 |     8 |     1 |     5 |     7 |     5 |      - |     317 |
| array_read          |    24 |     9 |     9 |     9 |     9 |    11 |    11 |    14 |      - |     222 |
| math_intensive      |    49 |    46 |    48 |    47 |    47 |    48 |    49 |    48 |      - |    1562 |
| object_create       |     3 |     0 |     0 |     0 |     0 |     5 |     5 |     5 |      - |     135 |
| nested_loops        |    43 |     8 |     8 |     8 |     8 |    10 |    18 |    18 |      - |     344 |
| accumulate          |    94 |    93 |    93 |    93 |    93 |    95 |    95 |    95 |      - |    4330 |

## Per-cell full stats

Format: median (p95: X, σ: S, min: Y, max: Z) ms

| Benchmark | Runtime | Stats (ms) |
|---|---|---|
| fibonacci | perry | 394 (p95: 420, σ: 7.4, min: 394, max: 420) |
| fibonacci | rust | 306 (p95: 329, σ: 6.6, min: 305, max: 329) |
| fibonacci | cpp | 298 (p95: 309, σ: 3.2, min: 297, max: 309) |
| fibonacci | go | 435 (p95: 454, σ: 5.6, min: 434, max: 454) |
| fibonacci | swift | 387 (p95: 409, σ: 6.3, min: 386, max: 409) |
| fibonacci | java | 273 (p95: 277, σ: 2.5, min: 269, max: 277) |
| fibonacci | node | 924 (p95: 1018, σ: 30.1, min: 923, max: 1018) |
| fibonacci | bun | 512 (p95: 515, σ: 2.4, min: 507, max: 515) |
| fibonacci | hermes | - |
| fibonacci | python | 12140 (p95: 13544, σ: 455.8, min: 12017, max: 13544) |
| loop_overhead | perry | 93 (p95: 116, σ: 6.6, min: 93, max: 116) |
| loop_overhead | rust | 93 (p95: 96, σ: 1.2, min: 93, max: 96) |
| loop_overhead | cpp | 93 (p95: 94, σ: 0.3, min: 93, max: 94) |
| loop_overhead | go | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| loop_overhead | swift | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| loop_overhead | java | 94 (p95: 95, σ: 0.5, min: 94, max: 95) |
| loop_overhead | node | 63 (p95: 64, σ: 0.4, min: 63, max: 64) |
| loop_overhead | bun | 39 (p95: 39, σ: 0.0, min: 39, max: 39) |
| loop_overhead | hermes | - |
| loop_overhead | python | 1941 (p95: 1949, σ: 2.9, min: 1940, max: 1949) |
| loop_data_dependent | perry | 218 (p95: 241, σ: 6.8, min: 217, max: 241) |
| loop_data_dependent | rust | 218 (p95: 224, σ: 2.2, min: 217, max: 224) |
| loop_data_dependent | cpp | 124 (p95: 126, σ: 0.6, min: 124, max: 126) |
| loop_data_dependent | go | 124 (p95: 124, σ: 0.0, min: 124, max: 124) |
| loop_data_dependent | swift | 217 (p95: 218, σ: 0.3, min: 217, max: 218) |
| loop_data_dependent | java | 220 (p95: 226, σ: 2.1, min: 219, max: 226) |
| loop_data_dependent | node | 220 (p95: 222, σ: 0.7, min: 219, max: 222) |
| loop_data_dependent | bun | 220 (p95: 223, σ: 1.2, min: 219, max: 223) |
| loop_data_dependent | hermes | - |
| loop_data_dependent | python | 5947 (p95: 5961, σ: 4.7, min: 5942, max: 5961) |
| array_write | perry | 2 (p95: 2, σ: 0.5, min: 1, max: 2) |
| array_write | rust | 6 (p95: 7, σ: 0.4, min: 5, max: 7) |
| array_write | cpp | 1 (p95: 1, σ: 0.0, min: 1, max: 1) |
| array_write | go | 8 (p95: 8, σ: 0.3, min: 7, max: 8) |
| array_write | swift | 1 (p95: 1, σ: 0.0, min: 1, max: 1) |
| array_write | java | 5 (p95: 5, σ: 0.4, min: 4, max: 5) |
| array_write | node | 7 (p95: 7, σ: 0.3, min: 6, max: 7) |
| array_write | bun | 5 (p95: 5, σ: 0.3, min: 4, max: 5) |
| array_write | hermes | - |
| array_write | python | 317 (p95: 320, σ: 1.2, min: 316, max: 320) |
| array_read | perry | 24 (p95: 34, σ: 2.9, min: 24, max: 34) |
| array_read | rust | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | cpp | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | go | 9 (p95: 14, σ: 1.5, min: 9, max: 14) |
| array_read | swift | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | java | 11 (p95: 11, σ: 0.5, min: 10, max: 11) |
| array_read | node | 11 (p95: 11, σ: 0.4, min: 10, max: 11) |
| array_read | bun | 14 (p95: 14, σ: 0.4, min: 13, max: 14) |
| array_read | hermes | - |
| array_read | python | 222 (p95: 225, σ: 1.0, min: 222, max: 225) |
| math_intensive | perry | 49 (p95: 71, σ: 6.4, min: 48, max: 71) |
| math_intensive | rust | 46 (p95: 48, σ: 0.6, min: 46, max: 48) |
| math_intensive | cpp | 48 (p95: 49, σ: 0.3, min: 48, max: 49) |
| math_intensive | go | 47 (p95: 48, σ: 0.3, min: 47, max: 48) |
| math_intensive | swift | 47 (p95: 48, σ: 0.3, min: 47, max: 48) |
| math_intensive | java | 48 (p95: 49, σ: 0.5, min: 48, max: 49) |
| math_intensive | node | 49 (p95: 50, σ: 0.4, min: 48, max: 50) |
| math_intensive | bun | 48 (p95: 49, σ: 0.5, min: 48, max: 49) |
| math_intensive | hermes | - |
| math_intensive | python | 1562 (p95: 1577, σ: 5.9, min: 1556, max: 1577) |
| object_create | perry | 3 (p95: 7, σ: 1.1, min: 3, max: 7) |
| object_create | rust | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | cpp | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | go | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | swift | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | java | 5 (p95: 5, σ: 0.5, min: 4, max: 5) |
| object_create | node | 5 (p95: 5, σ: 0.0, min: 5, max: 5) |
| object_create | bun | 5 (p95: 6, σ: 0.5, min: 5, max: 6) |
| object_create | hermes | - |
| object_create | python | 135 (p95: 145, σ: 2.9, min: 135, max: 145) |
| nested_loops | perry | 43 (p95: 60, σ: 4.9, min: 43, max: 60) |
| nested_loops | rust | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | cpp | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | go | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | swift | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | java | 10 (p95: 11, σ: 0.4, min: 10, max: 11) |
| nested_loops | node | 18 (p95: 18, σ: 0.4, min: 17, max: 18) |
| nested_loops | bun | 18 (p95: 19, σ: 0.5, min: 18, max: 19) |
| nested_loops | hermes | - |
| nested_loops | python | 344 (p95: 353, σ: 4.3, min: 339, max: 353) |
| accumulate | perry | 94 (p95: 110, σ: 4.7, min: 93, max: 110) |
| accumulate | rust | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| accumulate | cpp | 93 (p95: 94, σ: 0.3, min: 93, max: 94) |
| accumulate | go | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| accumulate | swift | 93 (p95: 95, σ: 0.6, min: 93, max: 95) |
| accumulate | java | 95 (p95: 97, σ: 0.8, min: 94, max: 97) |
| accumulate | node | 95 (p95: 96, σ: 0.6, min: 94, max: 96) |
| accumulate | bun | 95 (p95: 95, σ: 0.4, min: 94, max: 95) |
| accumulate | hermes | - |
| accumulate | python | 4330 (p95: 4522, σ: 55.2, min: 4327, max: 4522) |

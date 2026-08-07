# Polyglot Compute-Microbench Results (auto-generated)

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.5.0 arm64 on MacBookPro · **Date:** 2026-08-07
**Perry version:** v0.5.1335

Headline = median wall-clock ms. Lower is better.

| Benchmark           | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun | Hermes |  Python |
|---------------------|-------|-------|-------|-------|-------|-------|-------|-------|--------|---------|
| fibonacci           |   399 |   311 |   302 |   439 |   392 |   275 |   933 |   515 |      - |   12442 |
| loop_overhead       |    94 |    94 |    94 |    94 |    94 |    96 |    65 |    40 |      - |    1985 |
| loop_data_dependent |   222 |   220 |   126 |   126 |   221 |   222 |   224 |   223 |      - |    6114 |
| array_write         |     1 |     6 |     1 |     8 |     1 |     5 |     7 |     5 |      - |     329 |
| array_read          |    23 |     9 |     9 |     9 |     9 |    11 |    11 |    14 |      - |     231 |
| math_intensive      |    50 |    47 |    49 |    48 |    48 |    49 |    50 |    51 |      - |    1597 |
| object_create       |     2 |     0 |     0 |     0 |     0 |     5 |     5 |     6 |      - |     139 |
| nested_loops        |    41 |     8 |     8 |     8 |     8 |    10 |    18 |    19 |      - |     353 |
| accumulate          |    95 |    94 |    94 |    94 |    94 |    96 |    97 |    97 |      - |    4437 |

## Per-cell full stats

Format: median (p95: X, σ: S, min: Y, max: Z) ms

| Benchmark | Runtime | Stats (ms) |
|---|---|---|
| fibonacci | perry | 399 (p95: 407, σ: 2.6, min: 397, max: 407) |
| fibonacci | rust | 311 (p95: 317, σ: 1.7, min: 311, max: 317) |
| fibonacci | cpp | 302 (p95: 307, σ: 1.9, min: 300, max: 307) |
| fibonacci | go | 439 (p95: 445, σ: 2.1, min: 437, max: 445) |
| fibonacci | swift | 392 (p95: 395, σ: 1.3, min: 390, max: 395) |
| fibonacci | java | 275 (p95: 280, σ: 2.0, min: 273, max: 280) |
| fibonacci | node | 933 (p95: 945, σ: 5.4, min: 930, max: 945) |
| fibonacci | bun | 515 (p95: 523, σ: 3.4, min: 511, max: 523) |
| fibonacci | hermes | - |
| fibonacci | python | 12442 (p95: 12538, σ: 184.0, min: 12067, max: 12538) |
| loop_overhead | perry | 94 (p95: 96, σ: 0.7, min: 94, max: 96) |
| loop_overhead | rust | 94 (p95: 96, σ: 0.9, min: 93, max: 96) |
| loop_overhead | cpp | 94 (p95: 96, σ: 0.9, min: 93, max: 96) |
| loop_overhead | go | 94 (p95: 96, σ: 0.7, min: 94, max: 96) |
| loop_overhead | swift | 94 (p95: 96, σ: 0.8, min: 93, max: 96) |
| loop_overhead | java | 96 (p95: 98, σ: 0.9, min: 95, max: 98) |
| loop_overhead | node | 65 (p95: 65, σ: 0.4, min: 64, max: 65) |
| loop_overhead | bun | 40 (p95: 41, σ: 0.7, min: 39, max: 41) |
| loop_overhead | hermes | - |
| loop_overhead | python | 1985 (p95: 2021, σ: 11.3, min: 1979, max: 2021) |
| loop_data_dependent | perry | 222 (p95: 227, σ: 2.1, min: 220, max: 227) |
| loop_data_dependent | rust | 220 (p95: 226, σ: 2.0, min: 218, max: 226) |
| loop_data_dependent | cpp | 126 (p95: 128, σ: 1.1, min: 124, max: 128) |
| loop_data_dependent | go | 126 (p95: 129, σ: 1.4, min: 124, max: 129) |
| loop_data_dependent | swift | 221 (p95: 225, σ: 1.7, min: 218, max: 225) |
| loop_data_dependent | java | 222 (p95: 223, σ: 0.9, min: 220, max: 223) |
| loop_data_dependent | node | 224 (p95: 225, σ: 0.4, min: 224, max: 225) |
| loop_data_dependent | bun | 223 (p95: 224, σ: 1.1, min: 220, max: 224) |
| loop_data_dependent | hermes | - |
| loop_data_dependent | python | 6114 (p95: 6131, σ: 11.2, min: 6091, max: 6131) |
| array_write | perry | 1 (p95: 2, σ: 0.4, min: 1, max: 2) |
| array_write | rust | 6 (p95: 7, σ: 0.3, min: 6, max: 7) |
| array_write | cpp | 1 (p95: 1, σ: 0.0, min: 1, max: 1) |
| array_write | go | 8 (p95: 8, σ: 0.4, min: 7, max: 8) |
| array_write | swift | 1 (p95: 1, σ: 0.0, min: 1, max: 1) |
| array_write | java | 5 (p95: 5, σ: 0.4, min: 4, max: 5) |
| array_write | node | 7 (p95: 8, σ: 0.3, min: 7, max: 8) |
| array_write | bun | 5 (p95: 5, σ: 0.0, min: 5, max: 5) |
| array_write | hermes | - |
| array_write | python | 329 (p95: 331, σ: 2.1, min: 325, max: 331) |
| array_read | perry | 23 (p95: 23, σ: 0.3, min: 22, max: 23) |
| array_read | rust | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | cpp | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | go | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | swift | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | java | 11 (p95: 12, σ: 0.5, min: 10, max: 12) |
| array_read | node | 11 (p95: 11, σ: 0.5, min: 10, max: 11) |
| array_read | bun | 14 (p95: 14, σ: 0.3, min: 13, max: 14) |
| array_read | hermes | - |
| array_read | python | 231 (p95: 232, σ: 2.2, min: 225, max: 232) |
| math_intensive | perry | 50 (p95: 50, σ: 0.4, min: 49, max: 50) |
| math_intensive | rust | 47 (p95: 47, σ: 0.5, min: 46, max: 47) |
| math_intensive | cpp | 49 (p95: 50, σ: 0.5, min: 48, max: 50) |
| math_intensive | go | 48 (p95: 49, σ: 0.6, min: 47, max: 49) |
| math_intensive | swift | 48 (p95: 48, σ: 0.5, min: 47, max: 48) |
| math_intensive | java | 49 (p95: 50, σ: 0.5, min: 49, max: 50) |
| math_intensive | node | 50 (p95: 50, σ: 0.4, min: 49, max: 50) |
| math_intensive | bun | 51 (p95: 51, σ: 0.7, min: 49, max: 51) |
| math_intensive | hermes | - |
| math_intensive | python | 1597 (p95: 1605, σ: 3.4, min: 1594, max: 1605) |
| object_create | perry | 2 (p95: 3, σ: 0.4, min: 2, max: 3) |
| object_create | rust | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | cpp | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | go | 0 (p95: 1, σ: 0.3, min: 0, max: 1) |
| object_create | swift | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | java | 5 (p95: 5, σ: 0.3, min: 4, max: 5) |
| object_create | node | 5 (p95: 6, σ: 0.4, min: 5, max: 6) |
| object_create | bun | 6 (p95: 6, σ: 0.5, min: 5, max: 6) |
| object_create | hermes | - |
| object_create | python | 139 (p95: 140, σ: 1.4, min: 136, max: 140) |
| nested_loops | perry | 41 (p95: 46, σ: 1.6, min: 40, max: 46) |
| nested_loops | rust | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | cpp | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | go | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | swift | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | java | 10 (p95: 11, σ: 0.4, min: 10, max: 11) |
| nested_loops | node | 18 (p95: 19, σ: 0.4, min: 18, max: 19) |
| nested_loops | bun | 19 (p95: 20, σ: 0.4, min: 19, max: 20) |
| nested_loops | hermes | - |
| nested_loops | python | 353 (p95: 359, σ: 4.2, min: 348, max: 359) |
| accumulate | perry | 95 (p95: 96, σ: 0.9, min: 93, max: 96) |
| accumulate | rust | 94 (p95: 95, σ: 0.8, min: 93, max: 95) |
| accumulate | cpp | 94 (p95: 96, σ: 0.7, min: 93, max: 96) |
| accumulate | go | 94 (p95: 96, σ: 0.9, min: 93, max: 96) |
| accumulate | swift | 94 (p95: 96, σ: 0.8, min: 93, max: 96) |
| accumulate | java | 96 (p95: 98, σ: 0.9, min: 95, max: 98) |
| accumulate | node | 97 (p95: 99, σ: 1.0, min: 96, max: 99) |
| accumulate | bun | 97 (p95: 98, σ: 0.7, min: 96, max: 98) |
| accumulate | hermes | - |
| accumulate | python | 4437 (p95: 4478, σ: 13.2, min: 4425, max: 4478) |

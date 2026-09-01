# Polyglot Compute-Microbench Results (auto-generated)

**Runs per cell:** 11 · **Pinning:** Linux strict (taskset -c 0)
**Hardware:** Linux 6.17.0-23-generic x86_64 on ideal-mastodon · **Date:** 2026-09-01
**Perry version:** v0.5.1519

Headline = median wall-clock ms. Lower is better.

| Benchmark           | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun | Hermes |  Python |
|---------------------|-------|-------|-------|-------|-------|-------|-------|-------|--------|---------|
| fibonacci           |   323 |   189 |   103 |     - |     - |     - |   708 |   469 |      - |    8579 |
| loop_overhead       |    74 |    77 |    75 |     - |     - |     - |    76 |    83 |      - |    5629 |
| loop_data_dependent |   112 |   112 |   112 |     - |     - |     - |   115 |   124 |      - |    5425 |
| array_write         |     4 |     4 |     2 |     - |     - |     - |     5 |     9 |      - |     339 |
| array_read          |     6 |     6 |    12 |     - |     - |     - |     7 |    17 |      - |     193 |
| math_intensive      |    51 |    44 |    51 |     - |     - |     - |    44 |    55 |      - |    1240 |
| object_create       |     4 |     2 |     0 |     - |     - |     - |     7 |    20 |      - |     121 |
| nested_loops        |    26 |     5 |    10 |     - |     - |     - |    15 |    26 |      - |     330 |
| accumulate          |    58 |    63 |    56 |     - |     - |     - |   162 |    72 |      - |    3369 |

## Per-cell full stats

Format: median (p95: X, σ: S, min: Y, max: Z) ms

| Benchmark | Runtime | Stats (ms) |
|---|---|---|
| fibonacci | perry | 323 (p95: 325, σ: 0.8, min: 322, max: 325) |
| fibonacci | rust | 189 (p95: 194, σ: 2.0, min: 188, max: 194) |
| fibonacci | cpp | 103 (p95: 104, σ: 0.4, min: 103, max: 104) |
| fibonacci | go | - |
| fibonacci | swift | - |
| fibonacci | java | - |
| fibonacci | node | 708 (p95: 713, σ: 1.7, min: 706, max: 713) |
| fibonacci | bun | 469 (p95: 694, σ: 99.4, min: 440, max: 694) |
| fibonacci | hermes | - |
| fibonacci | python | 8579 (p95: 8853, σ: 110.8, min: 8484, max: 8853) |
| loop_overhead | perry | 74 (p95: 75, σ: 0.4, min: 74, max: 75) |
| loop_overhead | rust | 77 (p95: 80, σ: 2.4, min: 74, max: 80) |
| loop_overhead | cpp | 75 (p95: 75, σ: 0.3, min: 74, max: 75) |
| loop_overhead | go | - |
| loop_overhead | swift | - |
| loop_overhead | java | - |
| loop_overhead | node | 76 (p95: 79, σ: 0.9, min: 76, max: 79) |
| loop_overhead | bun | 83 (p95: 84, σ: 1.2, min: 81, max: 84) |
| loop_overhead | hermes | - |
| loop_overhead | python | 5629 (p95: 6106, σ: 194.1, min: 5511, max: 6106) |
| loop_data_dependent | perry | 112 (p95: 112, σ: 0.3, min: 111, max: 112) |
| loop_data_dependent | rust | 112 (p95: 118, σ: 2.4, min: 111, max: 118) |
| loop_data_dependent | cpp | 112 (p95: 113, σ: 0.6, min: 111, max: 113) |
| loop_data_dependent | go | - |
| loop_data_dependent | swift | - |
| loop_data_dependent | java | - |
| loop_data_dependent | node | 115 (p95: 116, σ: 0.5, min: 115, max: 116) |
| loop_data_dependent | bun | 124 (p95: 127, σ: 1.4, min: 122, max: 127) |
| loop_data_dependent | hermes | - |
| loop_data_dependent | python | 5425 (p95: 5869, σ: 180.7, min: 5375, max: 5869) |
| array_write | perry | 4 (p95: 4, σ: 0.5, min: 3, max: 4) |
| array_write | rust | 4 (p95: 5, σ: 0.9, min: 3, max: 5) |
| array_write | cpp | 2 (p95: 3, σ: 0.3, min: 2, max: 3) |
| array_write | go | - |
| array_write | swift | - |
| array_write | java | - |
| array_write | node | 5 (p95: 5, σ: 0.5, min: 4, max: 5) |
| array_write | bun | 9 (p95: 11, σ: 0.8, min: 9, max: 11) |
| array_write | hermes | - |
| array_write | python | 339 (p95: 342, σ: 1.2, min: 338, max: 342) |
| array_read | perry | 6 (p95: 6, σ: 0.5, min: 5, max: 6) |
| array_read | rust | 6 (p95: 6, σ: 0.0, min: 6, max: 6) |
| array_read | cpp | 12 (p95: 13, σ: 0.3, min: 12, max: 13) |
| array_read | go | - |
| array_read | swift | - |
| array_read | java | - |
| array_read | node | 7 (p95: 8, σ: 0.4, min: 7, max: 8) |
| array_read | bun | 17 (p95: 18, σ: 0.7, min: 16, max: 18) |
| array_read | hermes | - |
| array_read | python | 193 (p95: 325, σ: 59.1, min: 179, max: 325) |
| math_intensive | perry | 51 (p95: 55, σ: 4.1, min: 44, max: 55) |
| math_intensive | rust | 44 (p95: 45, σ: 0.5, min: 44, max: 45) |
| math_intensive | cpp | 51 (p95: 54, σ: 2.3, min: 48, max: 54) |
| math_intensive | go | - |
| math_intensive | swift | - |
| math_intensive | java | - |
| math_intensive | node | 44 (p95: 45, σ: 0.6, min: 43, max: 45) |
| math_intensive | bun | 55 (p95: 56, σ: 1.3, min: 52, max: 56) |
| math_intensive | hermes | - |
| math_intensive | python | 1240 (p95: 1978, σ: 332.2, min: 1110, max: 1978) |
| object_create | perry | 4 (p95: 4, σ: 0.4, min: 3, max: 4) |
| object_create | rust | 2 (p95: 2, σ: 0.0, min: 2, max: 2) |
| object_create | cpp | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | go | - |
| object_create | swift | - |
| object_create | java | - |
| object_create | node | 7 (p95: 8, σ: 1.2, min: 5, max: 8) |
| object_create | bun | 20 (p95: 21, σ: 1.7, min: 15, max: 21) |
| object_create | hermes | - |
| object_create | python | 121 (p95: 187, σ: 19.4, min: 118, max: 187) |
| nested_loops | perry | 26 (p95: 26, σ: 0.5, min: 25, max: 26) |
| nested_loops | rust | 5 (p95: 5, σ: 0.0, min: 5, max: 5) |
| nested_loops | cpp | 10 (p95: 11, σ: 0.4, min: 10, max: 11) |
| nested_loops | go | - |
| nested_loops | swift | - |
| nested_loops | java | - |
| nested_loops | node | 15 (p95: 18, σ: 1.4, min: 13, max: 18) |
| nested_loops | bun | 26 (p95: 27, σ: 1.9, min: 22, max: 27) |
| nested_loops | hermes | - |
| nested_loops | python | 330 (p95: 527, σ: 82.5, min: 319, max: 527) |
| accumulate | perry | 58 (p95: 62, σ: 1.4, min: 57, max: 62) |
| accumulate | rust | 63 (p95: 64, σ: 0.5, min: 62, max: 64) |
| accumulate | cpp | 56 (p95: 57, σ: 0.3, min: 56, max: 57) |
| accumulate | go | - |
| accumulate | swift | - |
| accumulate | java | - |
| accumulate | node | 162 (p95: 172, σ: 25.1, min: 101, max: 172) |
| accumulate | bun | 72 (p95: 76, σ: 1.7, min: 69, max: 76) |
| accumulate | hermes | - |
| accumulate | python | 3369 (p95: 5661, σ: 862.4, min: 3262, max: 5661) |

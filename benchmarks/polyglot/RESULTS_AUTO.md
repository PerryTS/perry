# Polyglot Compute-Microbench Results (auto-generated)

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.5.0 arm64 on MacBookPro · **Date:** 2026-08-06
**Perry version:** v0.5.1280

Headline = median wall-clock ms. Lower is better.

| Benchmark           | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun | Hermes |  Python |
|---------------------|-------|-------|-------|-------|-------|-------|-------|-------|--------|---------|
| fibonacci           |   417 |   312 |   302 |   458 |   389 |   273 |  1014 |   512 |      - |   12125 |
| loop_overhead       |    99 |    94 |    93 |    93 |    93 |    94 |    64 |    40 |      - |    1929 |
| loop_data_dependent |   229 |   220 |   125 |   124 |   217 |   219 |   239 |   224 |      - |    5940 |
| array_write         |     1 |     6 |     2 |     8 |     2 |     6 |     7 |     5 |      - |     319 |
| array_read          |    23 |     9 |     9 |    10 |     9 |    10 |    11 |    14 |      - |     227 |
| math_intensive      |    51 |    47 |    48 |    47 |    47 |    49 |    51 |    50 |      - |    1555 |
| object_create       |     2 |     0 |     0 |     0 |     0 |     4 |     5 |     6 |      - |     135 |
| nested_loops        |    41 |     8 |     8 |     9 |     8 |    10 |    19 |    19 |      - |     339 |
| accumulate          |   100 |    93 |    93 |    93 |    93 |    95 |   100 |    96 |      - |    4302 |

## Per-cell full stats

Format: median (p95: X, σ: S, min: Y, max: Z) ms

| Benchmark | Runtime | Stats (ms) |
|---|---|---|
| fibonacci | perry | 417 (p95: 464, σ: 24.1, min: 396, max: 464) |
| fibonacci | rust | 312 (p95: 334, σ: 6.4, min: 312, max: 334) |
| fibonacci | cpp | 302 (p95: 320, σ: 5.3, min: 300, max: 320) |
| fibonacci | go | 458 (p95: 545, σ: 32.3, min: 435, max: 545) |
| fibonacci | swift | 389 (p95: 407, σ: 5.5, min: 387, max: 407) |
| fibonacci | java | 273 (p95: 276, σ: 1.0, min: 272, max: 276) |
| fibonacci | node | 1014 (p95: 1136, σ: 73.8, min: 925, max: 1136) |
| fibonacci | bun | 512 (p95: 517, σ: 3.1, min: 506, max: 517) |
| fibonacci | hermes | - |
| fibonacci | python | 12125 (p95: 13054, σ: 279.8, min: 12112, max: 13054) |
| loop_overhead | perry | 99 (p95: 162, σ: 18.3, min: 96, max: 162) |
| loop_overhead | rust | 94 (p95: 96, σ: 0.9, min: 93, max: 96) |
| loop_overhead | cpp | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| loop_overhead | go | 93 (p95: 94, σ: 0.3, min: 93, max: 94) |
| loop_overhead | swift | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| loop_overhead | java | 94 (p95: 96, σ: 0.7, min: 94, max: 96) |
| loop_overhead | node | 64 (p95: 65, σ: 0.5, min: 64, max: 65) |
| loop_overhead | bun | 40 (p95: 40, σ: 0.0, min: 40, max: 40) |
| loop_overhead | hermes | - |
| loop_overhead | python | 1929 (p95: 2110, σ: 52.2, min: 1927, max: 2110) |
| loop_data_dependent | perry | 229 (p95: 420, σ: 55.9, min: 225, max: 420) |
| loop_data_dependent | rust | 220 (p95: 228, σ: 2.5, min: 218, max: 228) |
| loop_data_dependent | cpp | 125 (p95: 130, σ: 2.0, min: 124, max: 130) |
| loop_data_dependent | go | 124 (p95: 124, σ: 0.0, min: 124, max: 124) |
| loop_data_dependent | swift | 217 (p95: 218, σ: 0.3, min: 217, max: 218) |
| loop_data_dependent | java | 219 (p95: 220, σ: 0.4, min: 218, max: 220) |
| loop_data_dependent | node | 239 (p95: 275, σ: 15.9, min: 224, max: 275) |
| loop_data_dependent | bun | 224 (p95: 225, σ: 0.4, min: 224, max: 225) |
| loop_data_dependent | hermes | - |
| loop_data_dependent | python | 5940 (p95: 6166, σ: 64.7, min: 5934, max: 6166) |
| array_write | perry | 1 (p95: 2, σ: 0.5, min: 1, max: 2) |
| array_write | rust | 6 (p95: 7, σ: 0.4, min: 6, max: 7) |
| array_write | cpp | 2 (p95: 2, σ: 0.5, min: 1, max: 2) |
| array_write | go | 8 (p95: 9, σ: 0.6, min: 7, max: 9) |
| array_write | swift | 2 (p95: 2, σ: 0.0, min: 2, max: 2) |
| array_write | java | 6 (p95: 7, σ: 0.6, min: 5, max: 7) |
| array_write | node | 7 (p95: 8, σ: 0.4, min: 7, max: 8) |
| array_write | bun | 5 (p95: 5, σ: 0.3, min: 4, max: 5) |
| array_write | hermes | - |
| array_write | python | 319 (p95: 320, σ: 0.4, min: 319, max: 320) |
| array_read | perry | 23 (p95: 29, σ: 1.7, min: 23, max: 29) |
| array_read | rust | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | cpp | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | go | 10 (p95: 10, σ: 0.0, min: 10, max: 10) |
| array_read | swift | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | java | 10 (p95: 11, σ: 0.5, min: 10, max: 11) |
| array_read | node | 11 (p95: 15, σ: 1.2, min: 11, max: 15) |
| array_read | bun | 14 (p95: 14, σ: 0.4, min: 13, max: 14) |
| array_read | hermes | - |
| array_read | python | 227 (p95: 234, σ: 2.0, min: 227, max: 234) |
| math_intensive | perry | 51 (p95: 54, σ: 1.5, min: 50, max: 54) |
| math_intensive | rust | 47 (p95: 48, σ: 0.4, min: 46, max: 48) |
| math_intensive | cpp | 48 (p95: 48, σ: 0.0, min: 48, max: 48) |
| math_intensive | go | 47 (p95: 47, σ: 0.0, min: 47, max: 47) |
| math_intensive | swift | 47 (p95: 47, σ: 0.0, min: 47, max: 47) |
| math_intensive | java | 49 (p95: 49, σ: 0.5, min: 48, max: 49) |
| math_intensive | node | 51 (p95: 95, σ: 12.7, min: 49, max: 95) |
| math_intensive | bun | 50 (p95: 50, σ: 0.5, min: 49, max: 50) |
| math_intensive | hermes | - |
| math_intensive | python | 1555 (p95: 1558, σ: 1.2, min: 1553, max: 1558) |
| object_create | perry | 2 (p95: 3, σ: 0.5, min: 2, max: 3) |
| object_create | rust | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | cpp | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | go | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | swift | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | java | 4 (p95: 5, σ: 0.5, min: 4, max: 5) |
| object_create | node | 5 (p95: 6, σ: 0.5, min: 5, max: 6) |
| object_create | bun | 6 (p95: 6, σ: 0.4, min: 5, max: 6) |
| object_create | hermes | - |
| object_create | python | 135 (p95: 135, σ: 0.4, min: 134, max: 135) |
| nested_loops | perry | 41 (p95: 56, σ: 4.5, min: 40, max: 56) |
| nested_loops | rust | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | cpp | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | go | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| nested_loops | swift | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | java | 10 (p95: 11, σ: 0.3, min: 10, max: 11) |
| nested_loops | node | 19 (p95: 55, σ: 10.4, min: 18, max: 55) |
| nested_loops | bun | 19 (p95: 19, σ: 0.3, min: 18, max: 19) |
| nested_loops | hermes | - |
| nested_loops | python | 339 (p95: 339, σ: 0.5, min: 338, max: 339) |
| accumulate | perry | 100 (p95: 120, σ: 9.1, min: 96, max: 120) |
| accumulate | rust | 93 (p95: 94, σ: 0.4, min: 93, max: 94) |
| accumulate | cpp | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| accumulate | go | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| accumulate | swift | 93 (p95: 96, σ: 0.9, min: 93, max: 96) |
| accumulate | java | 95 (p95: 95, σ: 0.4, min: 94, max: 95) |
| accumulate | node | 100 (p95: 151, σ: 17.9, min: 96, max: 151) |
| accumulate | bun | 96 (p95: 97, σ: 0.5, min: 96, max: 97) |
| accumulate | hermes | - |
| accumulate | python | 4302 (p95: 4546, σ: 70.9, min: 4291, max: 4546) |

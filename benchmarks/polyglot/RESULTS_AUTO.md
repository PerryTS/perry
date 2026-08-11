# Polyglot Compute-Microbench Results (auto-generated)

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.5.0 arm64 on perry-macos · **Date:** 2026-08-08
**Perry version:** v0.5.1355

Headline = median wall-clock ms. Lower is better.

| Benchmark           | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun | Hermes |  Python |
|---------------------|-------|-------|-------|-------|-------|-------|-------|-------|--------|---------|
| fibonacci           |   390 |   308 |   300 |     - |   389 |   271 |   912 |   500 |      - |   24432 |
| loop_overhead       |    94 |    93 |    93 |     - |    93 |    95 |    63 |    39 |      - |    2599 |
| loop_data_dependent |   219 |   218 |   125 |     - |   218 |   221 |   221 |   221 |      - |   12630 |
| array_write         |     1 |     8 |     1 |     - |     1 |     5 |     7 |     5 |      - |     696 |
| array_read          |    22 |     9 |     9 |     - |     9 |    11 |    11 |    14 |      - |     323 |
| math_intensive      |    49 |    46 |    49 |     - |    47 |    49 |    49 |    49 |      - |    2272 |
| object_create       |     3 |     0 |     0 |     - |     0 |     5 |     5 |     5 |      - |     303 |
| nested_loops        |    40 |     8 |     8 |     - |     8 |    10 |    18 |    19 |      - |     559 |
| accumulate          |    94 |    93 |    93 |     - |    93 |    95 |    94 |    95 |      - |    4556 |

## Per-cell full stats

Format: median (p95: X, σ: S, min: Y, max: Z) ms

| Benchmark | Runtime | Stats (ms) |
|---|---|---|
| fibonacci | perry | 390 (p95: 391, σ: 0.4, min: 389, max: 391) |
| fibonacci | rust | 308 (p95: 350, σ: 12.1, min: 308, max: 350) |
| fibonacci | cpp | 300 (p95: 348, σ: 13.9, min: 299, max: 348) |
| fibonacci | go | - |
| fibonacci | swift | 389 (p95: 430, σ: 11.7, min: 389, max: 430) |
| fibonacci | java | 271 (p95: 272, σ: 0.4, min: 270, max: 272) |
| fibonacci | node | 912 (p95: 913, σ: 0.5, min: 912, max: 913) |
| fibonacci | bun | 500 (p95: 504, σ: 1.7, min: 499, max: 504) |
| fibonacci | hermes | - |
| fibonacci | python | 24432 (p95: 24445, σ: 8.8, min: 24414, max: 24445) |
| loop_overhead | perry | 94 (p95: 94, σ: 0.5, min: 93, max: 94) |
| loop_overhead | rust | 93 (p95: 94, σ: 0.3, min: 93, max: 94) |
| loop_overhead | cpp | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| loop_overhead | go | - |
| loop_overhead | swift | 93 (p95: 94, σ: 0.3, min: 93, max: 94) |
| loop_overhead | java | 95 (p95: 96, σ: 0.3, min: 95, max: 96) |
| loop_overhead | node | 63 (p95: 64, σ: 0.4, min: 63, max: 64) |
| loop_overhead | bun | 39 (p95: 40, σ: 0.4, min: 39, max: 40) |
| loop_overhead | hermes | - |
| loop_overhead | python | 2599 (p95: 2603, σ: 2.0, min: 2598, max: 2603) |
| loop_data_dependent | perry | 219 (p95: 220, σ: 0.3, min: 219, max: 220) |
| loop_data_dependent | rust | 218 (p95: 219, σ: 0.5, min: 218, max: 219) |
| loop_data_dependent | cpp | 125 (p95: 125, σ: 0.0, min: 125, max: 125) |
| loop_data_dependent | go | - |
| loop_data_dependent | swift | 218 (p95: 219, σ: 0.5, min: 218, max: 219) |
| loop_data_dependent | java | 221 (p95: 221, σ: 0.5, min: 220, max: 221) |
| loop_data_dependent | node | 221 (p95: 225, σ: 1.5, min: 221, max: 225) |
| loop_data_dependent | bun | 221 (p95: 221, σ: 0.0, min: 221, max: 221) |
| loop_data_dependent | hermes | - |
| loop_data_dependent | python | 12630 (p95: 12646, σ: 6.0, min: 12625, max: 12646) |
| array_write | perry | 1 (p95: 2, σ: 0.5, min: 1, max: 2) |
| array_write | rust | 8 (p95: 8, σ: 0.8, min: 6, max: 8) |
| array_write | cpp | 1 (p95: 2, σ: 0.4, min: 1, max: 2) |
| array_write | go | - |
| array_write | swift | 1 (p95: 2, σ: 0.5, min: 1, max: 2) |
| array_write | java | 5 (p95: 6, σ: 0.5, min: 5, max: 6) |
| array_write | node | 7 (p95: 7, σ: 0.0, min: 7, max: 7) |
| array_write | bun | 5 (p95: 5, σ: 0.0, min: 5, max: 5) |
| array_write | hermes | - |
| array_write | python | 696 (p95: 722, σ: 13.7, min: 672, max: 722) |
| array_read | perry | 22 (p95: 23, σ: 0.4, min: 22, max: 23) |
| array_read | rust | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | cpp | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | go | - |
| array_read | swift | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | java | 11 (p95: 11, σ: 0.4, min: 10, max: 11) |
| array_read | node | 11 (p95: 11, σ: 0.5, min: 10, max: 11) |
| array_read | bun | 14 (p95: 14, σ: 0.4, min: 13, max: 14) |
| array_read | hermes | - |
| array_read | python | 323 (p95: 329, σ: 2.6, min: 322, max: 329) |
| math_intensive | perry | 49 (p95: 49, σ: 0.0, min: 49, max: 49) |
| math_intensive | rust | 46 (p95: 47, σ: 0.3, min: 46, max: 47) |
| math_intensive | cpp | 49 (p95: 49, σ: 0.5, min: 48, max: 49) |
| math_intensive | go | - |
| math_intensive | swift | 47 (p95: 47, σ: 0.0, min: 47, max: 47) |
| math_intensive | java | 49 (p95: 49, σ: 0.4, min: 48, max: 49) |
| math_intensive | node | 49 (p95: 49, σ: 0.5, min: 48, max: 49) |
| math_intensive | bun | 49 (p95: 49, σ: 0.3, min: 48, max: 49) |
| math_intensive | hermes | - |
| math_intensive | python | 2272 (p95: 2316, σ: 16.1, min: 2254, max: 2316) |
| object_create | perry | 3 (p95: 3, σ: 0.4, min: 2, max: 3) |
| object_create | rust | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | cpp | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | go | - |
| object_create | swift | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | java | 5 (p95: 5, σ: 0.4, min: 4, max: 5) |
| object_create | node | 5 (p95: 5, σ: 0.4, min: 4, max: 5) |
| object_create | bun | 5 (p95: 6, σ: 0.5, min: 5, max: 6) |
| object_create | hermes | - |
| object_create | python | 303 (p95: 311, σ: 3.5, min: 300, max: 311) |
| nested_loops | perry | 40 (p95: 40, σ: 0.4, min: 39, max: 40) |
| nested_loops | rust | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | cpp | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | go | - |
| nested_loops | swift | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | java | 10 (p95: 11, σ: 0.5, min: 10, max: 11) |
| nested_loops | node | 18 (p95: 18, σ: 0.0, min: 18, max: 18) |
| nested_loops | bun | 19 (p95: 19, σ: 0.5, min: 18, max: 19) |
| nested_loops | hermes | - |
| nested_loops | python | 559 (p95: 566, σ: 3.4, min: 558, max: 566) |
| accumulate | perry | 94 (p95: 94, σ: 0.4, min: 93, max: 94) |
| accumulate | rust | 93 (p95: 94, σ: 0.3, min: 93, max: 94) |
| accumulate | cpp | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| accumulate | go | - |
| accumulate | swift | 93 (p95: 93, σ: 0.0, min: 93, max: 93) |
| accumulate | java | 95 (p95: 96, σ: 0.4, min: 95, max: 96) |
| accumulate | node | 94 (p95: 95, σ: 0.5, min: 94, max: 95) |
| accumulate | bun | 95 (p95: 96, σ: 0.3, min: 95, max: 96) |
| accumulate | hermes | - |
| accumulate | python | 4556 (p95: 4562, σ: 45.4, min: 4431, max: 4562) |

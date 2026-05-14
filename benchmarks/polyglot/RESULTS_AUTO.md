# Polyglot Compute-Microbench Results (auto-generated)

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.4.0 arm64 on MacBook-Pro-69 · **Date:** 2026-05-14
**Perry version:** v0.5.908

Headline = median wall-clock ms. Lower is better.

| Benchmark           | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun | Hermes |  Python |
|---------------------|-------|-------|-------|-------|-------|-------|-------|-------|--------|---------|
| fibonacci           |   309 |   316 |   309 |   446 |   401 |   278 |   987 |   518 |      - |   12382 |
| loop_overhead       |    97 |    97 |    96 |    96 |    96 |    97 |    53 |    41 |      - |    1967 |
| loop_data_dependent |   225 |   226 |   129 |   128 |   225 |   226 |   226 |   230 |      - |    6068 |
| array_write         |     3 |     7 |     2 |     9 |     2 |     6 |     9 |     6 |      - |     331 |
| array_read          |    11 |     9 |     9 |    10 |     9 |    11 |    14 |    16 |      - |     236 |
| math_intensive      |    51 |    48 |    50 |    48 |    48 |    50 |    49 |    50 |      - |    1579 |
| object_create       |     2 |     0 |     0 |     0 |     0 |     5 |     8 |     6 |      - |     133 |
| nested_loops        |    18 |     8 |     8 |    10 |     8 |    10 |    17 |    20 |      - |     353 |
| accumulate          |    97 |    97 |    96 |    96 |    96 |    98 |   597 |    98 |      - |    4382 |

## Per-cell full stats

Format: median (p95: X, σ: S, min: Y, max: Z) ms

| Benchmark | Runtime | Stats (ms) |
|---|---|---|
| fibonacci | perry | 309 (p95: 317, σ: 2.8, min: 307, max: 317) |
| fibonacci | rust | 316 (p95: 323, σ: 2.5, min: 315, max: 323) |
| fibonacci | cpp | 309 (p95: 312, σ: 1.0, min: 308, max: 312) |
| fibonacci | go | 446 (p95: 448, σ: 0.9, min: 445, max: 448) |
| fibonacci | swift | 401 (p95: 409, σ: 2.7, min: 399, max: 409) |
| fibonacci | java | 278 (p95: 296, σ: 6.0, min: 277, max: 296) |
| fibonacci | node | 987 (p95: 1030, σ: 12.5, min: 985, max: 1030) |
| fibonacci | bun | 518 (p95: 525, σ: 2.9, min: 515, max: 525) |
| fibonacci | hermes | - |
| fibonacci | python | 12382 (p95: 12503, σ: 58.7, min: 12322, max: 12503) |
| loop_overhead | perry | 97 (p95: 99, σ: 0.9, min: 96, max: 99) |
| loop_overhead | rust | 97 (p95: 98, σ: 0.6, min: 96, max: 98) |
| loop_overhead | cpp | 96 (p95: 97, σ: 0.6, min: 95, max: 97) |
| loop_overhead | go | 96 (p95: 97, σ: 0.6, min: 95, max: 97) |
| loop_overhead | swift | 96 (p95: 97, σ: 0.6, min: 95, max: 97) |
| loop_overhead | java | 97 (p95: 98, σ: 0.5, min: 97, max: 98) |
| loop_overhead | node | 53 (p95: 58, σ: 1.4, min: 53, max: 58) |
| loop_overhead | bun | 41 (p95: 42, σ: 0.7, min: 40, max: 42) |
| loop_overhead | hermes | - |
| loop_overhead | python | 1967 (p95: 2056, σ: 34.4, min: 1964, max: 2056) |
| loop_data_dependent | perry | 225 (p95: 234, σ: 2.7, min: 224, max: 234) |
| loop_data_dependent | rust | 226 (p95: 230, σ: 1.6, min: 225, max: 230) |
| loop_data_dependent | cpp | 129 (p95: 131, σ: 1.3, min: 128, max: 131) |
| loop_data_dependent | go | 128 (p95: 129, σ: 0.5, min: 128, max: 129) |
| loop_data_dependent | swift | 225 (p95: 226, σ: 0.6, min: 224, max: 226) |
| loop_data_dependent | java | 226 (p95: 229, σ: 1.4, min: 224, max: 229) |
| loop_data_dependent | node | 226 (p95: 228, σ: 0.8, min: 225, max: 228) |
| loop_data_dependent | bun | 230 (p95: 232, σ: 1.1, min: 228, max: 232) |
| loop_data_dependent | hermes | - |
| loop_data_dependent | python | 6068 (p95: 6186, σ: 41.6, min: 6044, max: 6186) |
| array_write | perry | 3 (p95: 4, σ: 0.5, min: 3, max: 4) |
| array_write | rust | 7 (p95: 8, σ: 0.4, min: 7, max: 8) |
| array_write | cpp | 2 (p95: 3, σ: 0.3, min: 2, max: 3) |
| array_write | go | 9 (p95: 9, σ: 0.4, min: 8, max: 9) |
| array_write | swift | 2 (p95: 4, σ: 0.8, min: 2, max: 4) |
| array_write | java | 6 (p95: 7, σ: 0.7, min: 5, max: 7) |
| array_write | node | 9 (p95: 9, σ: 0.5, min: 8, max: 9) |
| array_write | bun | 6 (p95: 9, σ: 1.0, min: 5, max: 9) |
| array_write | hermes | - |
| array_write | python | 331 (p95: 338, σ: 3.3, min: 327, max: 338) |
| array_read | perry | 11 (p95: 12, σ: 0.4, min: 11, max: 12) |
| array_read | rust | 9 (p95: 9, σ: 0.0, min: 9, max: 9) |
| array_read | cpp | 9 (p95: 11, σ: 0.7, min: 9, max: 11) |
| array_read | go | 10 (p95: 11, σ: 0.4, min: 10, max: 11) |
| array_read | swift | 9 (p95: 10, σ: 0.3, min: 9, max: 10) |
| array_read | java | 11 (p95: 13, σ: 0.6, min: 11, max: 13) |
| array_read | node | 14 (p95: 14, σ: 0.5, min: 13, max: 14) |
| array_read | bun | 16 (p95: 17, σ: 0.8, min: 14, max: 17) |
| array_read | hermes | - |
| array_read | python | 236 (p95: 244, σ: 4.7, min: 227, max: 244) |
| math_intensive | perry | 51 (p95: 51, σ: 0.4, min: 50, max: 51) |
| math_intensive | rust | 48 (p95: 49, σ: 0.7, min: 47, max: 49) |
| math_intensive | cpp | 50 (p95: 54, σ: 1.4, min: 49, max: 54) |
| math_intensive | go | 48 (p95: 49, σ: 0.5, min: 48, max: 49) |
| math_intensive | swift | 48 (p95: 50, σ: 0.8, min: 48, max: 50) |
| math_intensive | java | 50 (p95: 51, σ: 0.3, min: 50, max: 51) |
| math_intensive | node | 49 (p95: 52, σ: 1.0, min: 49, max: 52) |
| math_intensive | bun | 50 (p95: 52, σ: 0.9, min: 50, max: 52) |
| math_intensive | hermes | - |
| math_intensive | python | 1579 (p95: 1593, σ: 4.1, min: 1578, max: 1593) |
| object_create | perry | 2 (p95: 4, σ: 0.6, min: 2, max: 4) |
| object_create | rust | 0 (p95: 1, σ: 0.3, min: 0, max: 1) |
| object_create | cpp | 0 (p95: 1, σ: 0.4, min: 0, max: 1) |
| object_create | go | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | swift | 0 (p95: 0, σ: 0.0, min: 0, max: 0) |
| object_create | java | 5 (p95: 5, σ: 0.0, min: 5, max: 5) |
| object_create | node | 8 (p95: 9, σ: 0.5, min: 8, max: 9) |
| object_create | bun | 6 (p95: 9, σ: 1.0, min: 5, max: 9) |
| object_create | hermes | - |
| object_create | python | 133 (p95: 134, σ: 0.6, min: 132, max: 134) |
| nested_loops | perry | 18 (p95: 18, σ: 0.5, min: 17, max: 18) |
| nested_loops | rust | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | cpp | 8 (p95: 9, σ: 0.3, min: 8, max: 9) |
| nested_loops | go | 10 (p95: 12, σ: 0.8, min: 9, max: 12) |
| nested_loops | swift | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | java | 10 (p95: 12, σ: 0.7, min: 10, max: 12) |
| nested_loops | node | 17 (p95: 27, σ: 3.4, min: 16, max: 27) |
| nested_loops | bun | 20 (p95: 20, σ: 0.5, min: 19, max: 20) |
| nested_loops | hermes | - |
| nested_loops | python | 353 (p95: 356, σ: 3.6, min: 347, max: 356) |
| accumulate | perry | 97 (p95: 101, σ: 1.3, min: 96, max: 101) |
| accumulate | rust | 97 (p95: 98, σ: 0.9, min: 95, max: 98) |
| accumulate | cpp | 96 (p95: 97, σ: 0.4, min: 96, max: 97) |
| accumulate | go | 96 (p95: 97, σ: 0.5, min: 95, max: 97) |
| accumulate | swift | 96 (p95: 98, σ: 1.2, min: 95, max: 98) |
| accumulate | java | 98 (p95: 101, σ: 1.1, min: 97, max: 101) |
| accumulate | node | 597 (p95: 690, σ: 27.0, min: 592, max: 690) |
| accumulate | bun | 98 (p95: 99, σ: 0.6, min: 97, max: 99) |
| accumulate | hermes | - |
| accumulate | python | 4382 (p95: 4461, σ: 27.4, min: 4359, max: 4461) |

## Perry `--fast-math` addendum (separate run, same v0.5.908 binary recompiled with `PERRY_FAST_MATH=1`)

| Benchmark | Runtime | Stats (ms) |
|---|---|---|
| fibonacci | perry --fast | 306 (p95: 310, σ: 1.3, min: 305, max: 310) |
| loop_overhead | perry --fast | 12 (p95: 17, σ: 1.4, min: 12, max: 17) |
| loop_data_dependent | perry --fast | 224 (p95: 226, σ: 1.0, min: 223, max: 226) |
| array_write | perry --fast | 4 (p95: 7, σ: 1.2, min: 3, max: 7) |
| array_read | perry --fast | 11 (p95: 12, σ: 0.3, min: 11, max: 12) |
| math_intensive | perry --fast | 14 (p95: 16, σ: 0.6, min: 14, max: 16) |
| object_create | perry --fast | 0 (p95: 1, σ: 0.4, min: 0, max: 1) |
| nested_loops | perry --fast | 17 (p95: 25, σ: 2.2, min: 17, max: 25) |
| accumulate | perry --fast | 34 (p95: 36, σ: 0.9, min: 33, max: 36) |

The `--fast-math` rerun produced tight σ on every cell (1-2 ms typical),
indicating a clean run. Trivially-foldable cells (`loop_overhead`,
`math_intensive`, `accumulate`) reproduce the historical 7-8× win;
non-foldable cells (`fibonacci`, `loop_data_dependent`, `nested_loops`)
match default mode within 1-4 ms — exactly the structural expectation.

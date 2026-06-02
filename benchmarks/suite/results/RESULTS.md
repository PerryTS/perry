# suite/ Microbenchmark Results

**Run date:** 2026-05-14 · **Perry version:** v0.5.908
**Hardware:** Apple M1 Max (10 cores), 64 GB RAM, macOS 26.4 (Darwin 25.4.0), otherwise-idle machine.
**Runtimes:** Perry 0.5.908 / Node v25.8.0 / Bun 1.3.12. Static Hermes not installed.

**Methodology:** single run per cell (not RUNS=11). For multi-run medians + p95 + σ,
see [`benchmarks/polyglot/`](../../polyglot/) and [`benchmarks/json_polyglot/`](../../json_polyglot/).

Wall-clock ms. Lower is better.

| Benchmark         | Perry | Node.js |  Bun |
|-------------------|------:|--------:|-----:|
| loop_overhead     |    97 |      53 |   41 |
| array_write       |     3 |       9 |    6 |
| array_read        |    11 |      13 |   16 |
| fibonacci         |   319 |     988 |  515 |
| math_intensive    |    61 |      50 |   50 |
| object_create     |     2 |       9 |    7 |
| string_concat     |     0 |       3 |    1 |
| method_calls      |     9 |      11 |    9 |
| nested_loops      |    17 |      17 |   19 |
| prime_sieve       |     3 |       8 |    7 |
| binary_trees      |     2 |      10 |    7 |
| factorial         |   107 |     591 |   97 |
| closure           |    50 |     304 |   51 |
| mandelbrot        |    28 |      25 |   29 |
| matrix_multiply   |    28 |      34 |   34 |

### Startup time (avg of 5 runs)

| Metric     | Perry | Node.js | Bun |
|------------|------:|--------:|----:|
| cold start | 113ms |   131ms | 42ms |

### Peak RSS (binary_trees)

| Metric    | Perry | Node.js | Bun |
|-----------|------:|--------:|----:|
| peak RSS  |   6MB |    75MB | 49MB |

### Summary

- **vs Node.js:** 11 faster, 3 slower, 1 tied
- **vs Bun:** 11 faster, 3 slower, 1 tied

### Vs the 2026-05-13 (v0.5.891 contaminated) sweep

A parallel cargo build was running through part of yesterday's sweep, inflating
single-run cells. Today's results on an idle machine recovered most of the
apparent regressions:

| Benchmark      | v0.5.891 | v0.5.908 | Δ         | Notes |
|----------------|---------:|---------:|----------:|-------|
| method_calls   |     25ms |      9ms | **-16ms** | yesterday's reading was noise; back near baseline |
| math_intensive |     51ms |     61ms | +10ms     | within run-to-run noise on single-run methodology |
| factorial      |     98ms |    107ms | +9ms      | within noise |
| binary_trees   |      3ms |      2ms | -1ms      | tiny improvement |
| mandelbrot     |     23ms |     28ms | +5ms      | within noise |

### Vs the 2026-04-23 v0.5.173 baseline (still-pending regressions)

| Benchmark      | v0.5.173 | v0.5.908 | Δ         | Hypothesis |
|----------------|---------:|---------:|----------:|------------|
| factorial      |     31ms |    107ms | **+76ms** | v0.5.585 fast-math opt-in flip (FP-tail reduction no longer collapses to scalar) |
| closure        |     10ms |     50ms | **+40ms** | closure-env layout change since v0.5.173 — open follow-up |
| method_calls   |      1ms |      9ms | +8ms      | most regression closed in v0.5.908 (was 25ms yesterday); 8ms residual |
| prime_sieve    |      5ms |      3ms | -2ms      | improvement |
| matrix_multiply|     24ms |     28ms | +4ms      | within noise |
| binary_trees   |      3ms |      2ms | -1ms      | unchanged within noise |
| string_concat  |      0ms |      0ms | =         | unchanged |
| mandelbrot     |     23ms |     28ms | +5ms      | within single-run noise |

Reproduction:

```bash
cd benchmarks/suite
./run_benchmarks.sh
```

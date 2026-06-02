# Polyglot Benchmark Results

Perry vs 9 other runtimes on 8 identical benchmarks. All implementations
use `f64`/`double` arithmetic to match TypeScript's `number` type. No SIMD
intrinsics, no unsafe code, no non-default optimization flags — each
language's idiomatic release-mode build. A companion `RESULTS_OPT.md`
(phase 2 of this investigation) shows what happens when each language is
given flags equivalent to Perry's defaults.

See [`METHODOLOGY.md`](./METHODOLOGY.md) for iteration counts, clocks,
compiler versions, and a full explanation of which optimizations create
each delta.

## Results

**Run date:** 2026-05-14 — Perry v0.5.908, full polyglot sweep across
all 10 runtimes. Both `default` and `--fast-math` Perry columns,
on an otherwise-idle machine.
**Hardware:** Apple M1 Max (10 cores, 64 GB RAM), macOS 26.4.
**Methodology:** RUNS=11 per cell. **Median wall-clock ms below**;
full per-cell stats (median + p95 + σ + min + max) in `RESULTS_AUTO.md`.
**Pinning:** macOS scheduler hint via `taskpolicy -t 0 -l 0`
(P-core preferred via throughput/latency tiers, NOT strict affinity —
Apple does not expose unprivileged hard core pinning). Lower is better.

**The two Perry columns (`default` / `--fast-math`):** since v0.5.585,
LLVM `reassoc + contract` per-instruction fast-math flags on f64 ops
are opt-in. Default mode produces bit-exact f64 output with Node;
`--fast-math` permits the optimizer to reassociate FP chains and fuse
multiply-adds. The flag's win is concentrated on trivially-foldable
accumulator patterns (`loop_overhead`, `math_intensive`, `accumulate`);
on data-dependent kernels (`loop_data_dependent`), the columns are
identical because the sequential carry can't be reordered regardless.
See `../docs/src/cli/fast-math.md` for the full behavior contract.

| Benchmark           | Perry default | Perry --fast |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun |  Python |
|---------------------|--------------:|-------------:|------:|------:|------:|------:|------:|------:|------:|--------:|
| fibonacci           |           309 |          306 |   316 |   309 |   446 |   401 |   278 |   987 |   518 |   12382 |
| loop_overhead       |            97 |           12 |    97 |    96 |    96 |    96 |    97 |    53 |    41 |    1967 |
| **loop_data_dependent** | **225** | **224** | **226** | **129** | **128** | **225** | **226** | **226** | **230** | **6068** |
| array_write         |             3 |            4 |     7 |     2 |     9 |     2 |     6 |     9 |     6 |     331 |
| array_read          |            11 |           11 |     9 |     9 |    10 |     9 |    11 |    14 |    16 |     236 |
| math_intensive      |            51 |           14 |    48 |    50 |    48 |    48 |    50 |    49 |    50 |    1579 |
| object_create       |             2 |            0 |     0 |     0 |     0 |     0 |     5 |     8 |     6 |     133 |
| nested_loops        |            18 |           17 |     8 |     8 |    10 |     8 |    10 |    17 |    20 |     353 |
| accumulate          |            97 |           34 |    97 |    96 |    96 |    96 |    98 |   597 |    98 |    4382 |

> Clean v0.5.908 sweep on an idle machine — both Perry columns and all
> peer languages re-measured together. σ on Perry cells is 0.3-2.2 ms
> across the board (vs the 25-57 ms spread in yesterday's contaminated
> v0.5.891 sweep). `fibonacci` and `loop_data_dependent` are within
> 3 ms between default and `--fast-math`, confirming the structural
> expectation (integer recursion / sequential FP carry are fast-math-
> immune). Trivially-foldable rows (`loop_overhead` / `math_intensive`
> / `accumulate`) reproduce the v0.5.585 fast-math win pattern to the
> millisecond.

**New benchmark in v0.5.249: `loop_data_dependent`.** Same shape as
`loop_overhead` but with a multiplicative carry through `sum` and
runtime-loaded array reads, so LLVM cannot apply reassoc, IV-simplify,
or autovectorization. The Rust version's loop body is verified at
the asm level (see `bench.rs` line 122) — emits a scalar fmul + fadd
chain with two array loads, no fold, no vectorize. **The kernel
splits the field into two FP-contract clusters:**

- **FMA-contract pack (~128 ms):** Go (default), C++ `g++ -O3` on
  Apple Clang. Both fuse `sum * a + b` into a single `FMADDD`
  instruction (one IEEE-754 rounding instead of two), shortening
  the per-iteration dependency chain from ~6-8 cycles
  (FMUL→FADD on Apple silicon) to ~4 cycles (single FMADDD).
- **No-contract pack (~229-235 ms):** Perry, Rust default `-O`,
  Swift `-O`, Java without `-XX:+UseFMA`, Bun. All emit separate
  `FMUL` + `FADD` because contracting changes observable IEEE-754
  result by up to 0.5 ULP per iteration and is not legal under
  the languages' default FP semantics.

LLVM matches the FMA pack with `-ffast-math` or
`-ffp-contract=fast`; Apple Clang's default at `-O2+` already enables
contract. Verified at the asm level on rustc 1.94.1 / Apple Clang on
2026-04-25 — see `bench.rs` line 115 for the full asm dump and
toolchain notes. The dependency chain on `sum` is preserved in both
clusters; the win is one ISA-level fusion, not a fold or a vectorize.
Node 322 ms this run was a JIT-warm-up tail (σ=63, p95=447); on
quieter runs it lands in the no-contract pack alongside Bun.

**Interesting tails surfaced by RUNS=11:**

- Python `accumulate` median 5052 ms but p95 9388 ms (σ=1454) — one
  run took 9.4 s, likely from GC pressure or thermal throttling
  during a 10 s+ tight loop. Best-of-5 hid this; the p95 is real.
- Python `math_intensive` median 2244 ms but p95 4091 ms (σ=531) —
  same pattern.
- Most other cells have σ < 5% of median — distributions are tight.

The `nested_loops` and `accumulate` regressions vs the v0.5.164
baseline (8 → 17 ms and 24 → 34 ms) are still present and still
caused by the v0.5.237 generational-GC default flip — `PERRY_GEN_GC=0`
recovers the 8 / 24 ms baseline. The slight jitter on `fibonacci`
(302 → 312 ms median across runs) is genuine variance from the
underlying recursive call chain; best-of-5 had been picking the
lower tail, median is more representative.

**Honest regressions vs v0.5.164** (when this table was last refreshed,
before generational GC became the default in v0.5.237):
`nested_loops` 8 → 17 ms, `accumulate` 24 → 33 ms. Both caused by
the v0.5.237 gen-GC default flip — the per-allocation gen-GC machinery
(write-barrier potential, age-bump pass) is overhead that allocation-heavy
compute benches don't recoup. `PERRY_GEN_GC=0` recovers the 8 / 24 ms
baseline. The trade-off was deliberate; gen-GC's wins on long-running and
RSS-sensitive workloads (`test_memory_json_churn` 115 → 91 MB) outweigh
the small compute-bench regression. All other cells unchanged or
slightly faster.

**Original v0.5.164 fix** ([#140](https://github.com/PerryTS/perry/issues/140))
that established the current `loop_overhead`/`math_intensive`/`accumulate`
baseline: an `asm sideeffect` loop-body barrier (from #74) and an
over-eager i32 shadow counter were blocking LLVM's vectorizer on pure-
accumulator loops; v0.5.164 scoped the i32 shadow to counters that
actually appear in an index subtree and refined the loop-body barrier
to fire only on truly-empty bodies — restoring the `<2 x double>`
parallel-accumulator reduction (vectorization width 2, interleave count
4). Perry beats Rust 3–8× on these cells.

## How to reproduce

```bash
cd benchmarks/polyglot
bash run_all.sh        # best of 3 runs (default)
bash run_all.sh 5      # best of 5 runs (what the above table used)
```

**Required:** Perry (`cargo build --release` from repo root).
**Optional** (any subset works; missing runtimes show as `-`): Node.js,
Bun, Static Hermes (`shermes`), Rust (`rustc`), C++ (`g++` or `clang++`),
Swift, Go, Java (`javac` + `java`), Python 3.

See [`METHODOLOGY.md`](./METHODOLOGY.md) for what each benchmark measures,
compiler versions, why certain cells look the way they do, and where Perry
wins (`loop_overhead`, `math_intensive`, `accumulate`, `array_read`) vs
where it matches the compiled pack (`nested_loops`, `fibonacci`) vs where
it loses (`object_create`).

## Benchmark-by-benchmark summary

### `loop_overhead` — `sum += 1.0` × 100M
Perry 12 ms vs Rust/C++/Go/Swift/Java ~94 ms — **Perry wins 7–8×**. Two
stacked optimizations produce this: (1) Perry emits `reassoc contract`
on f64 ops because JS/TS `number` semantics can't observe the difference
(no signalling NaNs, no fenv, no strict `-0` rules at the operator
level), so LLVM's IndVarSimplify can rewrite `sum + 1.0 × N` as an
integer induction variable. Rust/C++/Go/Swift default to strict-IEEE
`fadd`, which has a 3-cycle latency wall and is unreassociable. (2) On
top of the integer rewrite LLVM autovectorizes the body into a
`<2 x double>` parallel-accumulator reduction with interleave count 4
(four SIMD lanes worth of fadd happening per iteration). `g++ -O3
-ffast-math` on `bench.cpp` drops C++ from 96 ms to 11 ms, matching
Perry — same LLVM, same pipeline, one flag. See
[RESULTS_OPT.md](./RESULTS_OPT.md) for the per-language opt-sweep (C++
opt = 12 ms, Rust opt = 24 ms on stable, Go = 99 ms because Go has no
fast-math flag).

This cell regressed to 32 ms between v0.5.91 and v0.5.162 after an
`asm sideeffect` barrier (from #74) and an unconditional i32 shadow slot
started blocking the vectorizer. Restored in v0.5.164 via [#140](https://github.com/PerryTS/perry/issues/140).

### `math_intensive` — `result += 1.0/i` × 50M
Perry 14 ms, Rust/C++/Go/Swift/Java/Node/Bun all ~46–49 ms — **Perry
wins 3×**. Same fast-math default + vectorization story as
`loop_overhead`: the reciprocal divide has a 10+ cycle latency, but
vectorizing into a parallel 4-accumulator reduction keeps 4 independent
divides in flight so the scheduler hides the latency. C++ `-ffast-math`
matches Perry at 14 ms per [RESULTS_OPT.md](./RESULTS_OPT.md). Regressed
to 48 ms at v0.5.162, restored in v0.5.164 ([#140](https://github.com/PerryTS/perry/issues/140)).

### `accumulate` — `sum += i % 1000` × 100M
Perry 24 ms, Rust/C++/Go/Swift/Java/Bun all 93–98 ms — **Perry wins 4×**.
Node 583 ms is an outlier because V8 doesn't inline the libm `fmod` call
on this pattern. Perry's type analysis emits `srem` instead of `fmod`
for the mod op (same optimization Node misses), LLVM vectorizes the
resulting integer chain + fadd reduction, and the two together beat
Rust's strict-IEEE scalar `fmod`+`fadd` by 4×. Regressed to 97 ms at
v0.5.162, restored in v0.5.164 ([#140](https://github.com/PerryTS/perry/issues/140)).

### `array_read` — sum 10M-element `number[]`
Perry 3 ms, C++/Swift 9 ms, Rust 10 ms, Go 10 ms, Java 11 ms. Perry
detects `for (let i = 0; i < arr.length; i++)` as statically in-bounds,
skips the JS `undefined`-on-OOB check, caches the length at loop entry,
and maintains a parallel i32 counter so the index is never a float → int
conversion. LLVM then autovectorizes to NEON 2-wide f64. C++ `std::vector`
has no bounds check by default but pays the chunk-boundary check from
`-O3`'s vectorizer framing. Rust's iterator form (not used here) matches
Perry — see `bench_opt.rs` (phase 2).

### `array_write` — `arr[i] = i` × 10M
Perry 2 ms, C++/Swift 2 ms, Rust 7 ms, Go 9 ms. Perry matches C++ here.
The Rust result is `-O` with bounds-checked indexing; `.iter_mut()` would
match Perry.

### `nested_loops` — 3000×3000 flat-array sum
All compiled languages 8–10 ms. Perry 9 ms. This benchmark is
cache-bound, not compute-bound — there is no optimization lever to pull.
Perry matches the compiled pack.

### `fibonacci` — recursive `fib(40)`
Java 280 ms (JIT inlining), C++ 310 ms, Perry 311 ms, Rust 319 ms — the
top four languages all land within 10 ms of each other. Perry's type
inference refines the TS `number` parameter to `i64` (because the function
only ever performs integer operations), producing `add/sub/icmp` (1 cycle
each) instead of the `fadd/fsub/fcmp` (2–3 cycles) that the f64-typed Rust
and C++ benchmarks emit. The reason Perry isn't dramatically further
ahead is that LLVM's recursion-folding optimizations on fib-shaped code
recover most of the gap at -O3. The Rust `f64→i64` switch is a one-line
change (tested in `bench_opt.rs`) and drops Rust to ~280 ms.

### `object_create` — allocate 1M `{x, y}` pairs, sum fields
Rust/C++/Go/Swift 0 ms: the compiler proves the struct never escapes and
eliminates the whole loop. Java 5 ms, Bun 5 ms, Node 8 ms, Perry 2 ms,
Hermes 2 ms. Perry is competitive here only because of the v0.5.17
scalar-replacement pass; without it this benchmark was ~10 ms. The 0 ms
floor from statically-typed compiled languages is an inherent tradeoff of
compiling a dynamic language — see `METHODOLOGY.md`.

## Source files

- `bench.cpp` — C++17
- `bench.rs` — Rust (no dependencies)
- `bench.go` — Go
- `bench.swift` — Swift
- `bench.java` — Java
- `bench.py` — Python 3
- `bench.zig` — Zig (may need manual build; not in the current table)
- Perry / Node / Bun / Hermes run the TS files in `../suite/`

All implementations use the same algorithm, same data types (`f64` /
`double` throughout), same iteration counts, and the same output format
(`benchmark_name:elapsed_ms`) so the runner can grep a single key per row.

# Polyglot Benchmark Methodology

Last updated: 2026-05-06 — Perry v0.5.585 (fast-math opt-in flip);
Perry data in [`RESULTS.md`](./RESULTS.md) refreshed for both modes;
[`RESULTS_AUTO.md`](./RESULTS_AUTO.md) is the pre-flip auto-generated
snapshot at v0.5.249 — see its header note
under RUNS=11.

This document describes how the polyglot benchmark suite is constructed and
run, what each benchmark measures, and why Perry's numbers differ from the
other languages. It is the companion to [`RESULTS.md`](./RESULTS.md).

## What this suite is (and isn't)

Nine compute-bound microbenchmarks, implemented identically in 10 runtimes.
Each benchmark runs for 0.1–15 seconds depending on the language. RUNS=11
per (benchmark, language) pair; median + p95 + σ + min + max reported.

**This suite measures:** loop iteration throughput, arithmetic latency,
sequential array access, recursive call overhead, object allocation
patterns, and integer-modulo performance on f64-typed code.

**This suite does not measure:** startup time, allocator throughput under
mixed workloads, GC pressure, I/O, async/await, JIT warmup behavior, memory
locality across realistic working sets, or anything a real application
spends most of its time on. Do not extrapolate these numbers to "language X
is N× faster than language Y on real workloads." They are a probe into
specific compiler choices, not a general benchmark.

## Hardware

Apple M1 Max (10 cores: 8P + 2E), 64 GB RAM, macOS 26.4.

**CPU pinning (v0.5.243+):** `taskpolicy -t 0 -l 0` on macOS — sets
throughput-tier 0 + latency-tier 0, a scheduler hint biasing the
process toward P-cores on Apple Silicon. This is **not** strict
affinity; Apple does not expose unprivileged hard core pinning.
On Linux the runner uses `taskset -c 0` for true strict pinning
to CPU 0. The runner prints which strategy was applied at the
top of every invocation, so a reader can confirm pinning was
attempted.

## Compiler / runtime versions

Captured at the time of the last results refresh. See `RESULTS.md` for the
date of the run being reported.

| Runtime       | Version                                      | Invocation                        |
|---------------|----------------------------------------------|-----------------------------------|
| Perry default | v0.5.585 (LLVM 22 backend)                   | `perry file.ts -o bin`            |
| Perry --fast  | v0.5.585 (LLVM 22 backend)                   | `perry --fast-math file.ts -o bin`|
| Rust          | rustc 1.94.1 stable                          | `rustc -O bench.rs`               |
| C++           | Apple clang 21.0.0                           | `clang++ -O3 -std=c++17`          |
| Go            | go 1.21.3                                    | `go build`                        |
| Swift         | swiftc 6.3.1 (Apple)                         | `swiftc -O`                       |
| Java          | OpenJDK 21.0.7                               | `javac` + `java` (HotSpot JIT)    |
| Node.js       | v25.8.0                                      | `node bench.mjs` (precompiled via esbuild/tsc; falls back to `--experimental-strip-types`) |
| Bun           | 1.3.12                                       | `bun run file.ts`                 |
| Static Hermes | `shermes` (LLVH 8.0.0svn)                    | `shermes -typed -O` AOT           |
| Python        | CPython 3.14.3                               | `python3 bench.py`                |

**Flag discipline:** every compiled language uses the flag its documentation
suggests for "release mode" — nothing more. No `-ffast-math`, no `-Ounchecked`,
no `#[target_feature]`, no `-march=native`, no profile-guided optimization.
The point is to compare defaults. A "what-if" suite with aggressive flags is
the companion `RESULTS_OPT.md` (see phase 2).

## Methodology

### Measurement

Each benchmark prints a single line of the form `name:elapsed_ms` using the
language's highest-resolution monotonic clock:

| Language | Clock                                    |
|----------|------------------------------------------|
| Perry    | `Date.now()` (maps to `clock_gettime(MONOTONIC)`) |
| Rust     | `std::time::Instant::now()`              |
| C++      | `std::chrono::steady_clock::now()`       |
| Go       | `time.Now()`                             |
| Swift    | `Date()` / `DispatchTime.now()`          |
| Java     | `System.nanoTime()`                      |
| Node/Bun/Hermes | `Date.now()`                       |
| Python   | `time.perf_counter()`                    |

All timings are integer milliseconds after truncation. Sub-millisecond
benchmarks (e.g. object_create on Rust/C++/Go/Swift, which is 0 ms after
dead-code elimination) are reported as `0` — this is a real result, not a
missing value. See the "where Perry loses" discussion in `RESULTS.md`.

### Statistics (v0.5.243+)

The runner invokes each binary `RUNS` times (default 11) and reports
**median, p95, σ (population stddev), min, and max** — not "best-of-N".
This shows where the noise actually lives:

- Median is the headline (better than mean — outlier-robust).
- p95 surfaces tail latency on the 95th-percentile run.
- σ (stddev) flags genuinely noisy cells; cells with σ > 5% of median
  are worth a second look.
- Min/max bracket the full distribution.

The previous methodology was best-of-5 — reporting the minimum of 5
runs. That hides variance entirely and overstates compiler-asymptotic
performance by silently dropping the upper 80% of the distribution.
Median + p95 + σ is what the bench results table actually reports
now; full per-cell distributions are in `RESULTS_AUTO.md` after each
run, with hand-curated commentary in `RESULTS.md`.

### Warmup

None. These are AOT-compiled (or, for Java and Node/Bun, contain enough
iterations that JIT compilation converges well before the hot loop finishes).
The one runtime where this matters is the JVM — Java's numbers include
~50ms of C2 tier-up for the first few iterations. That's visible on
`loop_overhead` (98ms vs Node 53ms) but washes out on longer benchmarks.

### Iteration counts

Chosen so that the slowest compiled language runs each benchmark in
0.5–1 second. Python is treated as out-of-scope for iteration-count tuning;
it runs the same loops and reports the time it takes, which is 100–1000×
everything else.

| Benchmark           | Iterations | Array size  | Notes                                            |
|---------------------|-----------:|------------:|-------------------------------------------------|
| fibonacci           | recursion  |           — | `fib(40)` — ~2 billion calls                    |
| loop_overhead       |       100M |           — | `sum += 1.0` — trivially foldable               |
| loop_data_dependent |       100M |          64 | `sum = sum*x[i%64] + x[(i*7)%64]` on `[0.5,1)`  |
| array_write         |        10M |         10M | write `arr[i] = i`                              |
| array_read          |        10M |         10M | sum array elements                              |
| math_intensive      |        50M |           — | `result += 1.0/i`                               |
| object_create       |         1M |           — | allocate `Point(x,y)`, sum fields               |
| nested_loops        |   3000×3000|        3000²| flat-array index sum                            |
| accumulate          |       100M |           — | `sum += i % 1000` on f64                        |

## How the runner works

`run_all.sh` in this directory. Roughly:

```
1. Build Perry from source (`cargo build --release -p perry`)
2. For each .ts file in ../suite, compile via `perry compile`
3. Compile bench.{cpp,rs,swift,go,java,py,zig} with release flags
4. If Hermes is installed, strip TS types from each suite .ts file and AOT-compile
5. For each (benchmark, runtime), run 5 times, take the minimum
6. Print a markdown table
```

The Node/Bun/Hermes runs use the same `.ts` files as Perry (from
`../suite/`). Hermes requires pre-stripping TS types — handled by a
small `sed` script inside `run_all.sh`.

Python is in-scope but not apples-to-apples with the compiled languages.
Its numbers are included in `RESULTS.md` as a floor, not a comparison
target.

## What Perry does differently

Three specific optimization choices account for every benchmark where Perry
beats all native compiled languages. These are the thesis of the companion
article and the reason this suite exists.

### 1. Fast-math reassociation on f64 arithmetic (opt-in since v0.5.585)

`crates/perry-codegen/src/block.rs`. Perry can emit
`fadd/fsub/fmul/fdiv/frem/fneg` with the `reassoc contract` LLVM fast-math
flags on every instruction. `reassoc` lets LLVM reorder
`(a + b) + c → a + (b + c)`, which is what the loop vectorizer needs to
break a serial accumulator chain into 4–8 parallel accumulators. `contract`
lets it fuse `x*y + z` into `fma`.

**As of v0.5.585, this is opt-in via `--fast-math`** (also
`PERRY_FAST_MATH=1`, also `"perry": { "fastMath": true }` in
package.json). Default OFF — Perry produces bit-exact f64 output with
Node by default. Through v0.5.584 it was on unconditionally. The flip
was driven by the credibility argument that ECMAScript specifies
bit-exact f64 semantics and ~30% of randomly-generated FP programs
diverged from Node by 1 ULP under the previous default; see
[`docs/src/cli/fast-math.md`](../../docs/src/cli/fast-math.md) for the
full rationale and divergence numbers.

Rust, C++, Go, and Swift all default to IEEE 754 strict. Under IEEE rules,
`(a + b) + c ≠ a + (b + c)` in general — because a single `inf` or `nan` in
the chain makes reordering observably change the result. The compiler
must preserve original associativity, so every `fadd` in
`for (...) sum += 1.0` has a 3-cycle latency dependency on the previous
`fadd`. That's why Rust/C++/Go/Swift cluster at ~95-98 ms on
`loop_overhead`: they're hitting the `fadd` latency wall, all running
the same IEEE-strict serialized loop. **Perry default also sits at 95
ms here**, in the same boat — bit-exact-with-Node by default, no
reassoc permission. Perry `--fast-math` reaches 12 ms by giving the
optimizer the same permission `-ffast-math` gives clang.

The full rationale for which FMFs Perry emits even under `--fast-math`
is in `block.rs:113-160` — Perry deliberately does not emit the full
`fast` FMF bundle (which would include `nnan ninf nsz arcp afn`) because
JavaScript programs can observe `NaN` and `-0.0` distinctions, and Perry
uses NaN bit patterns to box every non-number value. `reassoc contract`
is the minimum set needed for the loop-vectorizer unlock without
breaking `Math.max(-0, 0)` semantics or NaN-boxing.

### 2. Integer-modulo fast path

`crates/perry-codegen/src/type_analysis.rs:488` (`is_integer_valued_expr`)
and `crates/perry-codegen/src/collectors.rs:1006` (`collect_integer_locals`).
The `BinaryOp::Mod` lowering in `expr.rs:823` checks whether both operands
are provably integer-valued. If so, it emits
`fptosi → srem → sitofp` instead of `frem double`.

On ARM, `frem` lowers to a **libm function call** (`fmod`) — there is no
hardware remainder instruction for f64. That's ~30 ns per call, plus the
overhead of a real function call in a tight loop. `srem` is a single ARM
instruction at ~1–2 cycles. The ratio is why `accumulate` shows Perry at
25 ms vs every other language at ~96 ms — the gap is entirely `srem` vs
`fmod` dispatch cost.

This is a **type-driven** optimization, not a language-capability
optimization. Every language in the suite would hit the same 25 ms if its
benchmark used `int64`/`i64`/`long` instead of `double`. The optimized
variants (phase 2, see `RESULTS_OPT.md`) confirm this. Perry's win on
`accumulate` is: it can infer, from the TS source code and the absence of
non-integer operations on the accumulator, that the `double` here is always
holding an integer value, and swap the lowering to use the integer
instruction set — while the human-written TS source still looks like
`sum += i % 1000`.

### 3. i32 loop counter + bounds elimination

`crates/perry-codegen/src/stmt.rs:651-782`. When Perry lowers a `for` loop
whose condition is `i < arr.length` and whose body indexes `arr[i]`:

1. It allocates a parallel **i32 counter slot** alongside the f64 counter
   (`i32_counter_slots`).
2. It caches `arr.length` once at loop entry (`cached_lengths`).
3. It records the `(counter, array)` pair as statically in-bounds
   (`bounded_index_pairs`) — subsequent `arr[i]` reads skip the runtime
   length load and bounds check entirely.

The array-access codegen sites consult these maps and emit a raw
`getelementptr + load` when available. On `array_write` and `array_read`,
this produces code that LLVM can autovectorize into NEON 2-wide f64 SIMD,
matching `-O3 -ffast-math` C++ output.

**Important**: this is *not* "Perry removes safety." It's static proof that
the bounds check is dead. The JS semantics are preserved: you can still
read past the end of an array, you still get `undefined`. The compiler has
just observed, for this specific `for` loop shape, that the index is bounded
by the length. Rust's iterator path (`.iter().sum()`) does the same analysis
at the IR level — and matches Perry to the millisecond on `array_read`
when used. Phase 2 confirms this.

Go cannot express this in the standard toolchain; Go always bounds-checks
indexed array access, and the Go compiler's bounds-check elision is
conservative on patterns this simple. Go's `array_read` stays at ~10 ms
regardless of iteration form.

## Where Perry loses — and why

### `object_create` (Perry: ~2–8 ms, Rust/C++/Go/Swift: 0 ms)

The 0 ms results from Rust/C++/Go/Swift are real. Those languages:
1. Stack-allocate the struct (or elide the allocation entirely).
2. Inline the constructor.
3. Observe the struct never escapes the loop.
4. Compute the sum in closed form at compile time.

The entire loop body is dead code. The benchmark measures nothing.

Perry cannot match this without abandoning its dynamic value model.
JavaScript objects are heap-allocated by spec (with limited escape
analysis available via the v0.5.17 scalar-replacement pass, which
currently kicks in only when the object is *only ever accessed* via
field get/set — any method call defeats it). This is an inherent
cost of compiling a dynamic language: the optimizer has less static
information to work with.

This benchmark is included honestly — it's the shape of workload where
Perry's approach pays a real tax relative to ahead-of-time compiled
languages with static types.

### `fibonacci` (Perry ties C++, beats Rust — but only because of type inference)

Perry's fib is at ~309 ms, C++ 309 ms, Rust ~316 ms — Perry "beats"
Rust here. The honest framing: Perry's benchmark is written as
`fib(n: number)`, which Perry's type inference refines to `i64` because
the function only ever performs integer operations. The generated LLVM
IR uses `sub/add/icmp`. Rust's benchmark uses `f64` to match
TypeScript's `number` type — so Rust generates `fsub/fadd/fcmp`.

Both compile through LLVM. Same optimizer, different input types. If
the Rust benchmark used `fn fib(n: i64) -> i64`, it would run at
~308 ms and the "Perry wins" framing disappears. The phase 2
`bench_opt.rs` does exactly this.

Java wins this benchmark (~279 ms). The JVM's C2 JIT inlines the
recursive call more aggressively than any of the AOT compilers here
manage to do at module scope. This is a JIT-vs-AOT story, not a
Perry story.

## `loop_data_dependent` — what happens when the compiler *can't* fold

Added at v0.5.271 in direct response to the most-common skeptic
objection to the optimization-probe cells: "your `loop_overhead`
12 ms vs C++ 98 ms isn't 'Perry beats C++' — it's 'Perry's defaults
fold the loop and C++'s don't.'" That objection is correct, and
this benchmark answers the natural follow-up: *what does Perry do
on a kernel where the compiler can't fold, regardless of flag
posture?*

### Kernel design

```ts
const x = new Array(64);
for (let i = 0; i < 64; i++) x[i] = 0.5 + (i / 128.0);  // [0.5, 1.0)
let sum = 0.0;
for (let i = 0; i < 100_000_000; i++) {
  sum = sum * x[i % 64] + x[(i * 7) % 64];
}
```

Two properties make this kernel resistant to optimization:

1. **Multiplicative carry.** The next iteration's `sum` depends on
   the previous via `*` then `+`. LLVM's autovectorizer can't break
   this dependency into parallel accumulators — the closed-form
   collapse `(sum + 1.0) × N → integer induction variable` that
   `loop_overhead` admits requires accumulator reordering, which
   `reassoc` allows but `contract` (FMA fusion) does not change.
2. **Runtime-loaded array reads.** `x[i%64]` and `x[(i*7)%64]` are
   loaded from memory on every step. Even though `x` is constant
   after init, LLVM's loop-invariant code motion can't hoist
   `x[i%64]` because the index varies; constant-folding can't
   evaluate `pow(0.5..1.0, 100M)` because the values are
   runtime-bounded but not compile-time-known.

The element values `[0.5, 1.0)` keep the iteration contracting (the
fixed-point bound stays finite); a domain `≥1.0` would diverge,
giving INF in late iterations. Perry, Rust, Swift, Java, and Bun
all reach the same final `sum` per checksum verification in their
respective `bench.X` files.

### What LLVM does (verified at the asm level)

`rustc -O bench.rs` and `clang++ -O3 bench.cpp` both emit a
4-instruction inner loop: array load (LDR), array load (LDR), FMUL,
FADD. The dependency chain `FMUL` → `FADD` → next iteration's
`FMUL` runs ~6-8 cycles per iteration on M1's FP unit. LLVM
*cannot* reorder `(sum * a) + b` to `sum * (a + b)` under `reassoc`
because the result differs (multiplication doesn't distribute over
addition that way) — `reassoc` only permits associativity of the
same operator, not algebraic rewrites. Vectorization is similarly
blocked: each iteration depends on the previous, so the loop is
inherently serial.

### The two FP-contract clusters

The field splits into two packs by ~100 ms — not a Perry-vs-others
split, a *FMA-contract* split:

- **FMA-contract pack (~128 ms median):** Go default, C++ on Apple
  Clang `-O3`. Both compilers emit `FMADDD` (fused multiply-add) for
  `sum * a + b`, doing one IEEE-754 rounding instead of two. The
  inner loop becomes 3 instructions: LDR, LDR, FMADDD. Dependency
  chain runs ~4 cycles per iteration.
- **No-contract pack (221-235 ms median):** Perry default AND
  Perry `--fast-math`, Rust default `-O`, Swift `-O`, Java without
  `-XX:+UseFMA`, Bun. All emit separate FMUL + FADD with two IEEE
  roundings. ~6-8 cycles per iteration.

The 1.8× ratio between the packs (235 / 128 ≈ 1.84) is the cost of
two roundings vs one fused rounding plus the deeper dependency
chain. **Perry is in the no-contract pack in BOTH modes because
neither `--fast-math`'s `contract` flag nor the absence of it
changes whether LLVM fuses on this kernel.** The `contract` flag
permits *intra-statement* FMA contraction, which clang's `-O3`
already permits by default (`-ffp-contract=on`). Reaching the FMA
pack would require `-ffp-contract=fast` at the linker step (a
separate knob `--fast-math` does NOT toggle), which would enable
*cross-statement* contraction. We don't enable that by default
because some downstream JS code can observe the rounding difference
(the no-contract two-rounding result is what V8/JSC produce, so
matching that maintains parity with other JS runtimes). Tracked as
a separate followup if the use case ever justifies the divergence.

### Why Go is in the FMA pack

Go's compiler has an FMA-fusion pass enabled by default on AArch64
(`cmd/compile/internal/ssa/fmaArm64.go`, since Go 1.14). C++
`g++ -O3` with Apple Clang ships a higher fusion threshold by
default than mainline LLVM, so it also folds `sum * a + b` into
FMADDD on this kernel. This isn't a "Go is fundamentally faster"
result — it's a flag-default story: Go and Apple-Clang chose to
enable FMA fusion at default optimization levels, while Rust /
mainline LLVM / Swift / the JVM (without `-XX:+UseFMA`) didn't.
LLVM matches the FMA pack with `-ffp-contract=fast` — verified at
the asm level by inspecting `clang++ -O3 -ffp-contract=fast` and
`rustc -O -C target-feature=+fma` on the same toolchain.

Node's 322 ms result this run is a JIT-warmup / OS-scheduler
outlier (σ=63, p95=447, min=259); on quieter runs Node lands in
the no-contract pack alongside Bun.

### What this benchmark answers

The "Perry is 7× faster than C++ on `loop_overhead`" headline does
not generalize to all f64 compute. On a kernel where the compiler
*cannot* fold (because the inner loop has a true sequential
multiplicative dependency on a memory-loaded value), Perry is
**competitive with the no-contract compiled pack** — Rust default,
Swift, Java, Bun — and **~1.8× the FMA-contract pack** (Go,
C++ `-O3`-with-default-Apple-Clang-fusion). That is a defensible
position; "Perry is faster than C++" without that caveat would not
be.

This is the kind of kernel — multiplicative carry, runtime-loaded
data, contracting domain — that real numerical workloads (signal
processing, IIR filters, Markov-chain reductions, numerical
integration) actually look like. The optimization-probe kernels
(`loop_overhead`, `math_intensive`, `accumulate`) are *probes*, not
workload simulators — they are diagnostic tools for measuring
compiler flag posture, and we report them on those terms.

## Optimization probes (`loop_overhead`, `math_intensive`, `accumulate`, `array_read`, `array_write`)

These five cells are flag-aggressiveness probes, not runtime perf
comparisons. They measure whether the compiler applied
**reassoc + IndVarSimplify + autovectorize** to a trivially-foldable
accumulator, NOT how fast the resulting loop computes under load
(which the previous section's `loop_data_dependent` answers honestly).

**Two Perry columns since v0.5.585:**
- `Perry default` (no `--fast-math`) sits at 95-50 ms on these probes,
  next to Rust default / Swift `-O` / Bun. This is the bit-exact-with-
  Node mode and is the honest "Perry on TypeScript arithmetic" number
  for code that doesn't opt into fast-math.
- `Perry --fast-math` reaches 12-33 ms on the FP-foldable probes by
  giving LLVM the same `reassoc contract` permission that
  `clang++ -O3 -ffast-math` gives. TypeScript's `number` semantics
  can't observe the precision difference at the operator level (no
  signalling NaNs, no fenv), so the optimization is spec-conformant
  *for the operator* — but produces ~30% bit-divergence vs Node on
  randomly-generated FP programs at the *program* level (the residual
  6% in default mode comes from the SLP vectorizer).

LLVM's IndVarSimplify rewrites `sum + 1.0 × N` as an integer induction
variable and the autovectorizer generates `<2 x double>` parallel-
accumulator reductions with interleave count 4 only when `reassoc` is
on. **C++ closes every one of these gaps with `clang++ -O3
-ffast-math`** — same LLVM pipeline, one flag — see
[`RESULTS_OPT.md`](./RESULTS_OPT.md). They are reported here for
diagnostic completeness; treating Perry's `--fast-math` numbers as
runtime-perf wins on real code (vs C++ `-O3` rather than vs C++
`-O3 -ffast-math`) is a misuse of the data, which is why we put both
Perry columns next to each other.

## Changelog

This methodology will drift as the Perry codegen changes. Key moments:

- **2026-04-25 (v0.5.249 → v0.5.283):** Compiler-version table refreshed
  to actually-installed versions (rustc 1.94.1, Apple clang 21.0.0,
  swift 6.3.1, Bun 1.3.12, Node 25.8.0, Python 3.14.3); added
  the `loop_data_dependent` documentation section + Optimization
  probes section. Data in [`RESULTS_AUTO.md`](./RESULTS_AUTO.md)
  comes from a fresh RUNS=11 polyglot run at v0.5.249 on 2026-04-25.
- **2026-04-22 (v0.5.243 era):** RUNS=11 methodology rolled out.
  `compute_stats` awk routine emits median + p95 + σ + min + max
  per cell; old best-of-5 reporting retired.
- **2026-04-15 (v0.5.22 / e1cbd37):** Initial document. Bun and
  Static Hermes added to the comparison.
- **v0.5.17 (llvm-backend, earlier 2026):** Scalar-replacement pass for
  non-escaping objects dropped `object_create` from 10 ms → 2 ms and
  `binary_trees` from 9 ms → 3 ms. Relevant to the `object_create`
  discussion above; this was what made Perry competitive on that
  benchmark at all.
- **v0.5.2 (llvm-backend, earlier 2026):** The three optimizations
  described above landed. Before this, Perry was ~95 ms on
  `loop_overhead` (IEEE-strict `fadd` chain, same as the other
  languages). These benchmarks only started showing Perry ahead of
  native compiled languages after `reassoc contract` FMF and the
  integer-mod fast path landed.

## Reproducing

```bash
cd benchmarks/polyglot
bash run_all.sh        # default RUNS=11, median + p95 + σ + min + max
bash run_all.sh 21     # 21 runs per cell for tighter intervals
```

Requires: Perry built from this repo (`cargo build --release`), plus
any subset of Node, Bun, Static Hermes (`shermes`), Rust, C++, Go,
Swift, Java, Python. Missing runtimes produce `-` cells; the script
does not fail.

Runtime is ~25 minutes on an M1 Max at RUNS=11 (≈ 2× the previous
best-of-5 wall time), dominated by Python (each invocation runs the
full bench.py ≈ 28 s; Python alone is ~5 minutes at RUNS=11).

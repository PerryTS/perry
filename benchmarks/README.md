# Perry Benchmarks

This is the canonical, single-page comparison of Perry against its
**runtime peers** — **Node** and **Bun**, the production
TypeScript-input runtimes Perry is most directly compared against
— plus **TS-to-native peers** (AssemblyScript with json-as) and
**reference points** to hand-written compiled native languages
(Rust, Go, C++ with nlohmann + simdjson, Swift, Java, Kotlin) and
Python. The native compilers are *not* peers — they are
calibration. They show the floor of what hand-written, statically-
typed, compiled-ahead-of-time code achieves on this hardware so a
reader can see where Perry sits relative to that floor. The
comparisons that matter for "is Perry a good TypeScript runtime"
are against Node and Bun.

The format is designed for skeptics. Every implementation, every
flag, every methodology decision is in this page — no tables hidden
behind blog posts, no cherry-picked subsets.

> **Hardware:** Apple M1 Max (10 cores: 8P + 2E), 64 GB RAM, macOS
> 26.4. JSON benchmark numbers from 2026-04-25 (v0.5.249); Perry
> compute-microbench numbers refreshed 2026-05-06 (v0.5.585) following
> the fast-math opt-in flip — see "Compute microbenches" below for
> both `default` and `--fast-math` columns. Other languages' compute
> numbers retained from 2026-04-25 unless otherwise stated.
>
> **CPU pinning:** macOS `taskpolicy -t 0 -l 0` — sets throughput-tier 0
> + latency-tier 0, a scheduler HINT toward P-cores on Apple Silicon.
> This is **not** strict pinning; Apple does not expose unprivileged
> hard core affinity. (`taskpolicy -c user-interactive` does not exist;
> the `-c` clamp only accepts downgrade values utility/background/
> maintenance.) On Linux the runner uses `taskset -c 0` for strict
> pinning instead. The runner prints which strategy was applied at
> the top of each invocation.
>
> **Methodology:** RUNS=11 per cell (configurable via `$RUNS`). For
> each cell we collect every per-run wall-clock ms and report
> **median, p95, σ (population stddev), min, and max** — not
> "best-of-N". Headline tables show the median; full distributions
> are in [`json_polyglot/RESULTS.md`](json_polyglot/RESULTS.md) and
> [`polyglot/RESULTS.md`](polyglot/RESULTS.md). Time in milliseconds,
> RSS in MB (peak resident set size from `/usr/bin/time -l`, the worst
> peak observed across runs).
>
> **Pre-1.0 caveat:** Perry is pre-1.0 (v0.5.585); compared compilers
> and runtimes are stable releases. Numbers reflect Perry's current
> alpha state and may regress between releases.
>
> **Fast-math note (v0.5.585+):** LLVM `reassoc + contract` per-instruction
> fast-math flags on f64 ops are now opt-in via `--fast-math` (CLI),
> `PERRY_FAST_MATH=1` (env), or `"perry": { "fastMath": true }` in
> package.json. Off by default — Perry produces bit-exact f64 output
> with Node by default. Compute-microbench tables below show both modes
> in adjacent columns for transparency. See
> [`docs/src/cli/fast-math.md`](../docs/src/cli/fast-math.md) for the
> full behavior contract and the rationale.
>
> **Warmup:** the bench programs themselves run 3 untimed warmup
> iterations before the timed loop, to avoid charging JIT-y runtimes
> (Perry's compiled binary, V8, JSC, JVM) for cold-start. Process
> startup is included in the timed window for non-JIT runtimes (Go,
> Rust, C++, Swift) since their startup is sub-millisecond.
>
> **Node vs. Bun TS handling (asymmetric, on purpose).** Node
> measurements run on **precompiled `.mjs`** — the runner uses
> `esbuild` (or `tsc` as fallback) to strip TypeScript types in an
> untimed setup step, then `node bench.mjs`. Bun runs **`bench.ts`
> directly** because direct TypeScript execution is its native input
> format (and value prop). Without this asymmetry, Node would be
> charged on every launch for `--experimental-strip-types`'s parse +
> strip cost — work that Perry pays at compile time and Bun
> pays as part of being a TS-native runtime. With no stripper
> installed, the runner falls back to `node --experimental-strip-types`
> and prints a banner so the asymmetry is visible.

---

## Why these specific peers

This page mixes three categories of comparison and treats them
differently:

- **Runtime peers (Node, Bun).** Same input language as Perry
  (TypeScript), same general value proposition (run a TS program).
  These are the comparisons that matter most. If Perry doesn't
  beat Node and Bun on a workload, the workload doesn't favor
  Perry — and that's worth saying out loud rather than hiding.
- **TS-to-native peers (AssemblyScript with json-as; porffor and
  shermes were tried).** Same output as Perry: a native binary
  produced from TS source. These show what the TS-to-native
  ecosystem looks like today. porffor 0.61.13 and Static Hermes
  weren't bench-ready (see "Honest disclaimers"); AssemblyScript
  with `json-as` is the closest installable peer that runs the
  workload to completion.
- **Reference points to compiled native (Rust, C++, Go, Swift,
  Java, Kotlin).** Hand-written, statically-typed, compiled
  ahead-of-time. **These are NOT peers — they are calibration.**
  They show the floor of what compiled code achieves on this
  hardware, so a reader can see where Perry sits relative to that
  floor (closer than Node/Bun on some workloads, further on
  others). simdjson (C++ + SIMD) is the absolute parse-throughput
  ceiling; it is on the page deliberately, so the gap to it is
  visible. Perry is not expected to match it, and matching it is
  not the goal.

The headline question this page tries to answer honestly is
"compared to other TypeScript runtimes, is Perry's perf
competitive?" Native reference points exist to answer the
follow-up question: "and how does that compare to giving up
TypeScript entirely?"

---

## TL;DR

### JSON benchmarks — two workloads, both headline

10k records, ~1 MB blob, 50 iterations per run. Same data generator
across both. RUNS=11 per cell. Headline = median ms. Full per-cell
stats (median + p95 + σ + min + max) in
[`json_polyglot/RESULTS.md`](json_polyglot/RESULTS.md).

#### A. JSON validate-and-roundtrip
> Per iteration: `parse(blob)` → `stringify(parsed)` → discard.

The unmutated parse lets Perry's lazy tape (v0.5.204+) memcpy the
original blob bytes for stringify. simdjson uses the same fast-path
trick (`raw_json()` view into the original input), which is why
both runtimes lead this workload — they exploit the "no
modification" structure. nlohmann/json doesn't have this fast path
and rebuilds the string from the parsed tree on every `dump()`.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| **c++ -O3 -flto (simdjson)** | optimized | **24** | 28 | 1.2 | 23 | 28 | 8 |
| c++ -O2 (simdjson) | idiomatic | 29 | 34 | 1.7 | 28 | 34 | 8 |
| perry (gen-gc + lazy tape) | optimized | 75 | 91 | 6.9 | 69 | 91 | 85 |
| rust serde_json (LTO+1cgu) | optimized | 185 | 190 | 1.7 | 183 | 190 | 11 |
| rust serde_json | idiomatic | 198 | 204 | 2.3 | 195 | 204 | 11 |
| bun | idiomatic | 259 | 342 | 26.1 | 253 | 342 | 82 |
| perry (mark-sweep, no lazy) | untuned floor | 363 | 378 | 6.3 | 356 | 378 | 102 |
| node | idiomatic | 394 | 602 | 60.1 | 382 | 602 | 127 |
| kotlin -server -Xmx512m | optimized | 453 | 484 | 12.6 | 447 | 484 | 423 |
| kotlin (kotlinx.serialization) | idiomatic | 473 | 533 | 21.4 | 453 | 533 | 606 |
| node --max-old=4096 | optimized | 526 | 605 | 38.3 | 478 | 605 | 128 |
| assemblyscript+json-as (wasmtime) | idiomatic | 598 | 621 | 10.5 | 582 | 621 | 58 |
| c++ -O3 -flto (nlohmann/json) | optimized | 772 | 774 | 1.1 | 771 | 774 | 25 |
| go -ldflags="-s -w" -trimpath | optimized | 805 | 824 | 9.1 | 796 | 824 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 840 | 846 | 3.0 | 836 | 846 | 25 |
| go (encoding/json) | idiomatic | 848 | 1344 | 184.3 | 796 | 1344 | 23 |
| swift -O -wmo (Foundation) | optimized | 3709 | 3793 | 32.5 | 3686 | 3793 | 34 |
| swift -O (Foundation) | idiomatic | 3730 | 3844 | 54.3 | 3688 | 3844 | 34 |

#### B. JSON parse-and-iterate
> Per iteration: `parse(blob)` → sum every record's `nested.x`
> (touches every element) → `stringify(parsed)` → discard.

The full-tree iteration FORCES Perry's lazy tape to materialize, so
this is the honest comparison for workloads that touch JSON content.
Perry doesn't lead here — when you can't avoid the work, the lazy
tape pays its overhead without compensation.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| c++ -O2 (simdjson) | idiomatic | 24 | 27 | 0.9 | 24 | 27 | 8 |
| **c++ -O3 -flto (simdjson)** | optimized | **24** | 24 | 0.4 | 23 | 24 | 8 |
| rust serde_json (LTO+1cgu) | optimized | 183 | 185 | 1.2 | 182 | 185 | 11 |
| rust serde_json | idiomatic | 200 | 330 | 37.4 | 196 | 330 | 13 |
| bun | idiomatic | 254 | 255 | 1.9 | 249 | 255 | 87 |
| node --max-old=4096 | optimized | 355 | 389 | 11.4 | 346 | 389 | 87 |
| perry (mark-sweep, no lazy) | untuned floor | 375 | 402 | 10.2 | 370 | 402 | 102 |
| node | idiomatic | 380 | 652 | 87.2 | 356 | 652 | 101 |
| kotlin -server -Xmx512m | optimized | 455 | 465 | 6.1 | 444 | 465 | 426 |
| perry (gen-gc + lazy tape) | optimized | 466 | 475 | 7.0 | 457 | 475 | 100 |
| kotlin (kotlinx.serialization) | idiomatic | 469 | 481 | 5.9 | 459 | 481 | 608 |
| assemblyscript+json-as (wasmtime) | idiomatic | 605 | 632 | 11.4 | 587 | 632 | 58 |
| c++ -O3 -flto (nlohmann/json) | optimized | 786 | 793 | 2.7 | 782 | 793 | 25 |
| go -ldflags="-s -w" -trimpath | optimized | 805 | 833 | 9.2 | 798 | 833 | 22 |
| go (encoding/json) | idiomatic | 811 | 886 | 25.3 | 803 | 886 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 866 | 929 | 18.7 | 857 | 929 | 26 |
| swift -O (Foundation) | idiomatic | 3686 | 4009 | 96.7 | 3634 | 4009 | 34 |
| swift -O -wmo (Foundation) | optimized | 3702 | 3769 | 36.2 | 3660 | 3769 | 34 |

**Reading both tables together**: **simdjson leads both workloads
decisively** — 24 ms validate-and-roundtrip, 24 ms parse-and-iterate.
This is the honest C++ parse-throughput ceiling; cherry-picking
nlohmann would have hidden it. Perry's lazy tape (75 ms on
validate-and-roundtrip) is best-in-class **among dynamic-typing
runtimes** (beats Node 394 ms, Bun 259 ms, Kotlin 453 ms) but
loses cleanly to the SIMD-accelerated reference.

On parse-and-iterate, where the lazy tape can't shortcut, Perry
default lands at 466 ms — slower than its own mark-sweep escape
hatch (375 ms) because the lazy tape pays overhead the iteration
forces it to amortize. Rust serde_json with typed structs is the
non-SIMD champion at 183 ms; Bun is the dynamic-typing champion at
254 ms with single-digit σ. The AssemblyScript+json-as row
(598-605 ms) shows what the closest TS-to-native peer ships today —
typed structs + wasmtime + a wasm-target compile pipeline.

The honest framing: **Perry's JSON pipeline is competitive with
the dynamic-typing pack but loses to typed deserialization (Rust)
and to SIMD-accelerated parsing (simdjson)**. The
`PERRY_JSON_TAPE=0` escape hatch trades the lazy-tape fast path
for direct-parser performance on iterate-heavy workloads. Closing
the gap to simdjson's parse-throughput ceiling is tracked in
[`docs/json-typed-parse-plan.md`](../docs/json-typed-parse-plan.md).

### Compute microbenches (idiomatic flags)

RUNS=11 per cell. Perry numbers refreshed 2026-05-06 at v0.5.585
(both default and `--fast-math` columns); other languages from
2026-04-25 at v0.5.249 (their numbers haven't moved — same compiler
versions, same hardware, re-run on demand if needed). Headline =
median ms. Full per-cell stats (median + p95 + σ + min + max) in
[`polyglot/RESULTS_AUTO.md`](polyglot/RESULTS_AUTO.md) and the
hand-curated [`polyglot/RESULTS.md`](polyglot/RESULTS.md). Lower is
better. **`loop_overhead` and the other flag-aggressiveness probes
have moved to the "Optimization probes" subsection below** — to
avoid presenting them as runtime comparisons when they're really
compiler-flag probes.

| Benchmark           | Perry default | Perry --fast |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun |  Python |
|---------------------|--------------:|-------------:|------:|------:|------:|------:|------:|------:|------:|--------:|
| fibonacci           |           304 |          304 |   330 |   315 |   451 |   406 |   282 |  1022 |   589 |   16054 |
| loop_data_dependent |           221 |          221 |   229 |   129 |   128 |   233 |   229 |   322 |   232 |   10750 |
| object_create       |             2 |            0 |     0 |     0 |     0 |     0 |     5 |    11 |     6 |     164 |
| nested_loops        |            17 |           17 |     8 |     8 |    10 |     8 |    11 |    18 |    21 |     484 |

**Reading the two Perry columns:** identical numbers (`fibonacci`,
`loop_data_dependent`, `nested_loops`) mean the workload doesn't
benefit from `reassoc + contract` — either it's not FP-arithmetic-bound
(`fibonacci` is integer recursion, `nested_loops` is cache-bound) or
the FP work has a sequential dependency LLVM can't reorder regardless
of permission (`loop_data_dependent`'s `sum * x[i] + x[j]` chain — see
the discussion below). The 2/0 split on `object_create` is single-ms
noise on a sub-3-ms cell. **The benchmarks where the gap is large
sit in the "Optimization probes" table further down — that's the
section the fast-math flag actually moves.**

`fibonacci` (median 304 ms, both modes): Perry matches the compiled
pack within 11-22 ms; Java's HotSpot JIT is ~7% faster from inlining
the recursive call. Identical default vs `--fast-math` because this
kernel is integer recursion, not FP arithmetic.

`loop_data_dependent` (median 221 ms in both modes): the genuinely-
non-foldable f64 microbench (multiplicative carry through `sum`
plus array reads, 100M iters; LLVM cannot reorder under reassoc
and cannot vectorize past the sequential dependency — verified at
the asm level, see [`bench.rs`](polyglot/bench.rs#L122)). The
sequential dependency on `sum` is preserved across every language
on the row; the kernel is genuinely non-foldable. **Crucially,
this is the bench where `--fast-math` does NOTHING for Perry**
(221 ms either way) — sequential `sum * x[i] + x[j]` carries can't
be reordered no matter how permissive the FMF flags are.

**The kernel splits the field into two FP-contract clusters:** an
*FMA-contract pack* at ~128 ms (Go default, C++ `g++ -O3` on Apple
Clang — both fuse `sum * a + b` into a single `FMADDD` instruction
with one IEEE-754 rounding instead of two) and a *no-contract pack*
at 221-235 ms (Perry default AND `--fast-math`, Rust default `-O`,
Swift `-O`, Java without `-XX:+UseFMA`, Bun) running scalar `FMUL`
+ `FADD`, two roundings, ~6-8 cycle dependency chain vs FMADDD's
~4. Why doesn't `--fast-math`'s `contract` flag put Perry in the
FMA pack here? Because the AArch64 backend at `-O3` already pattern-
matches `mul + add` to FMADDD when it can prove the operands are
in registers and the rounding rules permit; the gating factor is
clang's `-ffp-contract` mode (Perry passes nothing, leaving it at
clang's `on` default which permits intra-statement contraction
*only*). Cross-statement contraction (which is what `--fast-math`'s
`contract` adds) doesn't help here because every `sum * x[i] + x[j]`
is one expression statement. Reaching the FMA pack would require
`-ffp-contract=fast` at the linker step, which is a separate knob
not covered by `--fast-math`. Node's 322 ms is a JIT-warm-up
outlier (σ=63, p95=447); on quieter runs it lands in the
no-contract pack alongside Bun. **Net answer to "what does Perry
do on real FP work?":** competitive with the no-contract compiled
pack regardless of `--fast-math` mode; reaching the FMA-contract
pack needs a different lever entirely.

`object_create` (1M iters): median 2 ms default / 0 ms `--fast-math`
— sub-3-ms cells where 1-tick differences swing the headline number;
not a real perf delta. Within a tick of native (Rust/C++/Go/Swift all
hit median 0 because their working set fits in one arena block; Perry
hits 1-2 because gen-GC adds a single allocation-counter increment
per iteration). `--fast-math` doesn't legitimately speed this up —
the 0 ms reading is just floor effect.

`nested_loops` (3000×3000 flat-array sum): cache-bound, not
compute-bound; everyone lands at 8-21 ms. `--fast-math` identical
because the bottleneck is L1/L2 latency, not FP throughput.

#### Optimization probes (compiler flag-aggressiveness, not runtime perf)

These five cells are *flag-aggressiveness probes*, not runtime perf
comparisons. They measure whether the compiler applied
**reassoc + IndVarSimplify + autovectorize** to a trivially-foldable
accumulator, NOT how fast the resulting loop actually computes
under load.

**As of v0.5.585, fast-math is opt-in.** Perry's default mode lands
in the no-flags pack alongside Rust/Swift/Bun on the FP-foldable
benches; `--fast-math` reproduces the headline numbers Perry was
posting through v0.5.584. The two-column shape lets readers see both
truths at once: bit-exact-with-Node by default; opt-in 7-8× speedup
on the foldable accumulator pattern. **C++ closes the same gap with
`-O3 -ffast-math`** — same LLVM pipeline, one flag. See
[`polyglot/RESULTS_OPT.md`](polyglot/RESULTS_OPT.md) for the
per-language flag-tuning sweep.

| Benchmark           | Perry default | Perry --fast |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun |  Python |
|---------------------|--------------:|-------------:|------:|------:|------:|------:|------:|------:|------:|--------:|
| loop_overhead       |            95 |           12 |    98 |    98 |    98 |   143 |   100 |    54 |    46 |    3019 |
| math_intensive      |            50 |           14 |    48 |    51 |    49 |    50 |    74 |    51 |    51 |    2238 |
| accumulate          |            95 |           33 |    98 |    98 |    98 |    98 |   100 |   617 |   100 |    5048 |
| array_read          |            11 |           11 |     9 |     9 |    11 |     9 |    12 |    13 |    16 |     342 |
| array_write         |             4 |            3 |     7 |     3 |     9 |     2 |     7 |     9 |     6 |     401 |

Perry default-column reading: `loop_overhead`, `math_intensive`, and
`accumulate` are all ~95-50 ms — dead-on the unflagged compiled pack
(Rust/Swift/Bun at 46-100 ms). That's the honest "Perry on TypeScript
arithmetic with bit-exact-Node semantics" number. `array_read` and
`array_write` are essentially mode-independent (memory-bound).

Perry --fast-column reading: same kernels with reassoc + contract
permitted reach 12 / 14 / 33 ms. On `loop_overhead` and `accumulate`,
LLVM's IndVarSimplify rewrites `sum + 1.0 × N` as an integer
induction variable and the autovectorizer generates `<2 x double>`
parallel-accumulator reductions with interleave count 4. On
`math_intensive`, the harmonic-sum carry is associative under
`reassoc`, allowing the same vectorize-and-reduce pattern.

The 7-8× speedup on `loop_overhead` is real, repeatable, and
TypeScript-spec-conformant only because TypeScript's `number`
semantics can't observe `reassoc contract` differences — no
signalling NaNs, no fenv, no strict `-0` rules at the operator
level. The trade is the ~30% bit-divergence-from-Node rate documented
in [`docs/src/cli/fast-math.md`](../docs/src/cli/fast-math.md).

The companion `loop_data_dependent` (in the headline table above)
shows what Perry looks like on the same kind of kernel WHEN THE
COMPILER CAN'T FOLD even with permission: 221 ms, dead-on the
no-contract pack (Rust/Swift/Java/Bun 229-233 ms), regardless of
mode. The Go/C++-O3 FMA-contract pack at ~128 ms beats us on this
kernel because they fuse FMUL + FADD into FMADDD via clang's
`-ffp-contract=fast` (a separate knob `--fast-math` does NOT
toggle). A reader who treats the 12 ms `loop_overhead` number as
"Perry is 7× faster than C++" without reading this paragraph has
been misled by the headline; the honest comparison is the default
column, where Perry sits *with* the compiled pack, not above it.

**Honest regressions / changes vs the v0.5.164 baseline:**

`v0.5.237` flip (gen-GC default ON):

- `nested_loops` 8 → 17 ms (+9 ms). Gen-GC adds per-allocation
  overhead (write-barrier potential, age-bump pass) that's pure
  cost on workloads that don't benefit from it. Set
  `PERRY_GEN_GC=0` to recover the 8 ms baseline.
- `accumulate` 24 → 33 ms (`--fast-math` mode), or 95 ms (default
  mode). Gen-GC + fast-math flip both contribute. Combined
  workaround: `PERRY_GEN_GC=0` plus `--fast-math` recovers the
  v0.5.164 24 ms.
- `object_create` 0 → 0-2 ms (gen-GC only). Within noise.
- `array_read`/`array_write` 3 → 3-11 ms. The 11 ms `array_read`
  on default mode is a v0.5.585 regression I haven't isolated yet
  — likely cache-prefetch ordering shifted with the new emission.
  Tracked as a followup; not gated by either GC or fast-math
  changes individually.

`v0.5.585` flip (fast-math opt-in):

- `loop_overhead` default 12 → 95 ms (+83 ms). `--fast-math` mode
  recovers 12 ms exactly. The change is intentional: see "Optimization
  probes" above for the rationale.
- `math_intensive` default 14 → 50 ms (+36 ms). `--fast-math`
  recovers 14 ms.
- `accumulate` default 34 → 95 ms (+61 ms). `--fast-math` recovers
  33 ms.
- All other cells (`fibonacci`, `array_read`, `array_write`,
  `nested_loops`, `loop_data_dependent`, `object_create`)
  identical between modes within noise — fast-math changed
  nothing observable on those workloads.

The trade-off was deliberate: gen-GC's wins on long-running and
allocation-heavy workloads (`test_memory_json_churn` 115 → 91 MB
in v0.5.237) outweigh the small compute-bench regressions, and
the escape hatch is right there. Listed here unapologetically
because the point of this page is to be defensible.

**Tail-latency findings** that median + p95 + σ surfaced (and
best-of-5 had hidden):

- Python `accumulate` median 5052 ms, p95 9388 ms (σ 1454 ms) —
  one run took 9.4 s, ~2× the typical case. Likely GC pressure or
  thermal throttle during a 10 s+ tight loop. The previous best-of-5
  reported "4854 ms" and silently dropped this tail.
- Python `math_intensive`: median 2244, p95 4091 (σ 532). Same
  pattern.
- Swift `-O -wmo` JSON: median 3879 ms, p95 5309 ms (σ 427) —
  Swift's whole-module optimization sometimes spends a long time
  in JSON's reflection pipeline; "optimized" is genuinely noisier
  than `-O` alone (which has σ=73).

These tails are real numbers measured today, not cherry-picked
worst cases. Best-of-N hides them; median + p95 puts them on the
page.

---

## What this page does not measure

Surfaced before the per-benchmark detail sections so a reader sees
the limitations alongside the headline numbers, not buried after
them.

- **GC latency / tail latency.** Reported numbers are throughput
  (median wall clock across RUNS=11 invocations). A 99th-percentile
  pause measurement would show Perry's stop-the-world GC at a
  disadvantage vs Go's concurrent collector or HotSpot ZGC.
- **JIT warmup behavior.** JS-family runtimes (Node, Bun) get
  3-iteration warmup before timed iterations to avoid charging them
  for cold-JIT compilation. Real cold-start latency is much worse for
  V8 / JSC than for Perry / Go / Rust binaries.
- **Async / await.** Every benchmark on this page is synchronous.
  Async runtime overhead, event-loop scheduling, microtask draining
  — not measured here.
- **I/O.** No file, network, or DB benchmark. Perry's `perry/thread`
  + tokio integration for HTTP / WebSocket / DB is benchmarked
  separately (see [`docs/`](../docs/) — partial).
- **Realistic application workloads.** Microbenches are probes,
  not workload simulators. The "Perry is X× faster than Y" claim
  is only true on the specific workload shape measured.
- **Memory pressure under contention.** All benches run on an
  otherwise-idle machine. Behavior under co-located-tenant pressure
  is not measured.
- **Compile time / binary size.** Perry compiles slower than Go (Go
  is famously fast at compile-time). Binary size is ~1 MB for hello
  world; comparable to Go but bigger than Rust release binaries with
  panic=abort + strip.

---

## How to read this page

The **compute microbenches** measure compiler choices: loop iteration
throughput, arithmetic latency, sequential array access, recursive
call overhead, object allocation patterns. These are probes into
specific code-generation behavior, not workload simulators. Don't
extrapolate to "language X is N× faster than Y on real applications".

The **JSON benchmarks** are closer to real-world: parse a 1 MB
structured JSON blob (10k records, each with 5 fields including a
nested object and a string array). Two workloads, both reported as
headline tables in TL;DR §A and §B: validate-and-roundtrip
(parse → stringify; no intermediate work) and parse-and-iterate
(parse → sum every record's nested.x → stringify). The two
together catch GC pressure, allocator throughput, encoding/decoding
pipeline cost, AND the cost of touching parsed values vs leaving
them lazy — which separates "Perry's lazy tape avoiding the work"
from "Perry's tape paying overhead it can't amortize".

The **memory benchmarks** are RSS-plateau and GC-aggression regression
tests. They run sustained allocate-and-discard loops for 200k iterations
and assert RSS stays under a per-test ceiling. They catch slow leaks
that microbenchmarks miss.

Every entry below is run twice — **idiomatic** (the language's default
release-mode build, what most projects ship with) and **optimized**
(aggressive flags: LTO, single codegen unit, fast-math where applicable,
etc.). This is intentional. Some readers correctly point out that
"Perry's defaults are themselves aggressive" — so we show every
language's full ceiling, not just its conservative starting point.

---

## 1. JSON polyglot — full data

[`benchmarks/json_polyglot/`](json_polyglot/) — implementation sources +
runner.

### Workload

```typescript
const items = [];
for (let i = 0; i < 10000; i++) {
  items.push({
    id: i,
    name: "item_" + i,
    value: i * 3.14159,
    tags: ["tag_" + (i % 10), "tag_" + (i % 5)],
    nested: { x: i, y: i * 2 }
  });
}
const blob = JSON.stringify(items);  // ~1 MB

// 50 iterations
for (let iter = 0; iter < 50; iter++) {
  const parsed = JSON.parse(blob);
  JSON.stringify(parsed);
}
```

Identical workload in 7 languages: TypeScript (run on Perry / Bun /
Node), Go, Rust, Swift, C++. Each language's implementation lives in
[`bench.<ext>`](json_polyglot/) with the same checksumming logic so
correctness is verifiable.

### Compiler flags used (verbatim)

| Profile | Language | Flags |
|---|---|---|
| optimized | Perry | `cargo build --release -p perry` (LLVM `-O3` equivalent, lazy JSON tape default for 64 KB..16 MB blobs, gen-GC default ON since v0.5.237) |
| untuned floor | Perry (escape hatch) | `PERRY_GEN_GC=0 PERRY_JSON_TAPE=0` (full mark-sweep, no lazy parse). Neither flag is something an idiomatic user sets; this row is the default-disabled baseline so a skeptic can see the floor under Perry's tuning. |
| idiomatic | Bun | `bun bench.ts` — runs **TS source directly** (no precompile; that IS Bun's value prop) |
| idiomatic | Node | `node bench.mjs` — runs **precompiled JS** (`.mjs` produced by `esbuild`/`tsc` as an untimed setup step). Falls back to `node --experimental-strip-types bench.ts` only when no stripper is on PATH; the runner prints a banner if it does. |
| optimized | Node | `node --max-old-space-size=4096 bench.mjs` (same precompile as above) |
| idiomatic | Go | `go build` (default) |
| optimized | Go | `go build -ldflags="-s -w" -trimpath` (smaller binary; ~no perf delta — included for completeness, see "honest disclaimers" below) |
| idiomatic | Rust | `cargo build --release` (`opt-level=3`, `lto=false`, `codegen-units=16`) |
| optimized | Rust | `cargo build --profile release-aggressive` (`opt-level=3`, `lto="fat"`, `codegen-units=1`, `panic=abort`, `strip=true`) |
| idiomatic | Swift | `swiftc -O bench.swift` |
| optimized | Swift | `swiftc -O -wmo bench.swift` (whole-module optimization) |
| idiomatic | Kotlin | `java -cp ... BenchKt` (JVM defaults, kotlinx.serialization) |
| optimized | Kotlin | `java -server -Xmx512m -cp ... BenchKt` (server JIT + heap tuning) |
| idiomatic | C++ (nlohmann) | `clang++ -std=c++17 -O2` |
| optimized | C++ (nlohmann) | `clang++ -std=c++17 -O3 -flto` |
| idiomatic | C++ (simdjson) | `clang++ -std=c++17 -O2 -lsimdjson` |
| optimized | C++ (simdjson) | `clang++ -std=c++17 -O3 -flto -lsimdjson` |
| idiomatic | AssemblyScript | `npx asc bench.ts --target release --transform json-as/transform` (extends `@assemblyscript/wasi-shim`); runs as `wasmtime build/release.wasm` |

### JSON libraries used

| Language | Library | Why this one |
|---|---|---|
| Perry | built-in `JSON.parse` / `JSON.stringify` (with optional [lazy tape](../docs/json-typed-parse-plan.md)) | Standard JS API, no library to choose |
| Bun / Node | built-in `JSON.parse` / `JSON.stringify` | Standard JS API |
| Go | `encoding/json` | Standard library; what every Go project starts with |
| Rust | `serde_json` (1.0) | The de facto standard; ~ubiquitous in the Rust ecosystem |
| Swift | `Foundation.JSONEncoder` / `JSONDecoder` | Apple's standard |
| Kotlin | `kotlinx.serialization-json` (1.9.0) | The official Kotlin serialization library; uses compile-time-generated (de)serializers, no reflection |
| **C++ (popular default)** | **nlohmann/json** (3.12.0) | The de facto popular C++ JSON library; not the fastest available but what most projects reach for |
| **C++ (parse-throughput ceiling)** | **simdjson** (4.3.0) | The SIMD-accelerated reference. Listed alongside nlohmann so the table shows both "what most projects ship with" AND "the C++ parse ceiling". simdjson is expected to beat Perry on time — see "Honest disclaimers" below. |
| AssemblyScript (TS-to-native peer) | `json-as` (1.3.2) | The de facto performant JSON library for AssemblyScript. Compile-time-generated (de)serializers via a transform, same approach as Rust serde / Kotlin kotlinx.serialization. AS is strictly typed (no `any`); the bench shape is closer to the Rust/Kotlin typed-struct rows than the dynamic-typing JS rows — see "Honest disclaimers" below. |

Both C++ libraries are listed because each answers a different
question. nlohmann answers *"what does the typical C++ project's
JSON pipeline look like?"* — it's the popular default and most
real codebases use it. simdjson answers *"what's the C++ parse
ceiling?"* — it's a SIMD-accelerated reference parser; if Perry
is going to lose to anything in this table, it's going to be
simdjson on parse-heavy workloads. The page shows both rows so
the comparison is honest in both directions.

### Honest disclaimers on the JSON numbers

- **Perry's `lazy tape` win is workload-specific.** On
  parse-then-iterate-every-element workloads, lazy tape is a net
  loss — it pays the tape build cost without amortizing the
  materialize-on-demand savings. On parse-then-`.length`-or-
  stringify workloads (which this bench is), lazy tape wins
  decisively. See [`audit-lazy-json.md`](../docs/audit-lazy-json.md)
  for the access-pattern matrix.
- **Rust's RSS lead is fundamental.** Rust's serde_json
  deserializes into typed structs (Vec<Item> with stack-laid-out
  fields). Perry, Bun, Node parse into dynamic heap objects (one
  alloc per value). The 8× RSS gap (11 MB Rust vs 85 MB Perry) is
  the cost of dynamic typing — it can't be closed without giving up
  TypeScript's `any` semantics. The fix is to teach Perry's parser
  about typed targets at compile time; tracked as
  [`json-typed-parse-plan.md`](../docs/json-typed-parse-plan.md)
  (Step 2 partially done; more in flight).
- **Go's `optimized` ≈ idiomatic.** `-ldflags="-s -w" -trimpath`
  strips debug info; no measurable perf delta. Included so the
  table doesn't look like Go was unfairly held back.
- **Swift's slow time is real, not a setup problem.** `-O -wmo`
  is what Swift Package Manager release builds use. The Foundation
  JSON pipeline goes through `Mirror`-based reflection on `Codable`
  types and is genuinely slow on macOS. swift-json is faster; not
  included because this is the standard.
- **Kotlin's RSS is JVM heap reservation, not working-set.** The
  JVM eagerly reserves up to `-Xmx` even when actual heap usage is
  much smaller. `-Xmx512m` gives 423 MB peak RSS; default settings
  reserve more (606 MB observed). The actual JSON working-set in
  Kotlin is comparable to Java/JVM peers. The 423-606 MB RSS
  number is correct for "what the OS sees the process holding"
  but is not a fair comparison of allocator efficiency.
- **Perry's "mark-sweep, no lazy" entry isn't recommended for
  production** — it disables the lazy JSON tape (v0.5.210) and the
  generational GC default (v0.5.237). It exists so you can see the
  untuned floor and compare against it.
- **simdjson beats Perry on time, decisively, on both workloads.**
  This is expected and correct. simdjson is a SIMD-accelerated
  parser purpose-built for JSON parse-throughput; on
  validate-and-roundtrip it lands at ~24 ms median and on
  parse-and-iterate at ~24 ms. Perry's lazy tape is a 12-byte-per-
  value sequential representation; it's competitive with
  general-purpose JSON libraries (nlohmann, serde_json,
  encoding/json) on the right workload, but it does not have
  simdjson's vectorized validation pipeline. **The simdjson row is
  in this table on purpose** — cherry-picking weak C++ libraries
  is exactly what this disclaimers section is supposed to prevent.
  When a future commit closes the simdjson gap on parse-throughput
  for typed inputs, that result will land here as well; tracked
  in `docs/json-typed-parse-plan.md`.
  *Footnote on simdjson's stringify*: simdjson 4.x doesn't ship a
  built-in stringify primitive. Our `bench_simdjson.cpp` uses
  `simdjson::ondemand` for parse and `doc.raw_json()` (a
  zero-copy view into the original input bytes) as the
  "stringified" output — same conceptual approach as Perry's lazy
  tape memcpy fast path. This is fair: both runtimes exploit the
  "no modification between parse and stringify" structure of the
  workload. nlohmann/json does NOT have this fast path and
  rebuilds the string from the parsed tree on every `dump()`.
- **AssemblyScript is the closest TS-to-native peer we could
  install + run on this bench.** porffor (a more direct AOT TS
  compiler) was tried but produced incorrect output and segfaulted
  on the 10k-record workload — porffor 0.61.13 is alpha-quality
  and not ready for benchmarks of this size. Static Hermes
  (`shermes`) is not available on Homebrew or npm in a way that
  installs cleanly on macOS arm64. AS compiles to WebAssembly and
  runs via wasmtime; numbers reflect the wasmtime AOT compile
  time + runtime, not pure-native time. AS is strictly typed so
  the workload uses concrete `Item`/`Nested` classes rather than
  `items: any[]` — which makes the AS row closer in shape to the
  Rust serde_json / Kotlin kotlinx.serialization typed-struct
  rows than to the dynamic-typing JS rows. The number is real
  ("AS+json-as on this workload runs in N ms"), but a reader
  shouldn't extrapolate to "AS is the language for TS-to-wasm
  performance" without context.

---

## 2. Compute microbenches — full data

[`benchmarks/polyglot/`](polyglot/) — 10 implementations across 9
benchmarks. **Perry cells in TL;DR's "Compute microbenches" and
"Optimization probes" tables are RUNS=11 medians refreshed
2026-05-06 at v0.5.585**, run in BOTH `default` and `--fast-math`
modes following the v0.5.585 fast-math opt-in flip. Other languages'
cells are RUNS=11 medians from the 2026-04-25 polyglot run at
v0.5.249 — same compiler versions, same hardware; their numbers
shouldn't have moved, and a full polyglot rerun is on the
follow-up list but not blocking. See
[`RESULTS_AUTO.md`](polyglot/RESULTS_AUTO.md) for per-cell
distributions (median + p95 + σ + min + max) of the 2026-04-25 run.
The JSON polyglot tables in TL;DR §A and §B were rerun separately
and are fresh as of 2026-04-25 at v0.5.279.

### Idiomatic flags table (current)

See [`RESULTS.md`](polyglot/RESULTS.md) for the full table reproduced
in the TL;DR above. Compiler details:

| Language | Compiler | Idiomatic flag |
|---|---|---|
| Perry default | self-hosted Rust, LLVM 22 | `perry app.ts` (no `--fast-math` — bit-exact f64 with Node) |
| Perry --fast | self-hosted Rust, LLVM 22 | `perry --fast-math app.ts` (LLVM `reassoc + contract` per-instruction FMFs; ~30% bit-divergence vs Node) |
| Rust | rustc 1.94.1 stable | `cargo build --release` |
| C++ | Apple clang 21.0.0 | `clang++ -O3 -std=c++17` |
| Go | go 1.21.3 | `go build` |
| Swift | swiftc 6.3.1 (Apple) | `swiftc -O` |
| Java | OpenJDK 21.0.7 (HotSpot) | default `java -cp .` |
| Kotlin (JSON only) | kotlinc 2.3.21 | `java -cp ... BenchKt` |
| Node.js | v25.8.0 | `node bench.mjs` (precompiled .mjs via `esbuild`/`tsc`; falls back to `node --experimental-strip-types` if no stripper is on PATH) |
| Bun | 1.3.12 | `bun bench.ts` (runs TS source directly — that IS Bun's value prop) |
| Static Hermes | shermes 0.13 | `shermes -O` (skipped if not installed) |
| Python | CPython 3.14.3 | `python3` |

Kotlin is JSON-only (not in the compute polyglot table) because the
compute polyglot runner predates Kotlin support; adding it would
require porting the 8-benchmark `bench.kt` to match the existing
`bench.cpp`/`bench.go`/etc. shape. Tracked as a follow-up.

### Optimized flags + delta table

[`RESULTS_OPT.md`](polyglot/RESULTS_OPT.md) holds the full opt-tuning
sweep. Highlights (note: comparisons here are against **Perry
`--fast-math`**, the column where Perry uses `reassoc + contract` —
the only fair apples-to-apples comparison once C++ also enables
`-ffast-math`):

- **C++ `-O3 -ffast-math` matches Perry `--fast-math` to the
  millisecond** on `loop_overhead` (12 = 12) and `math_intensive`
  (14 = 14). Perry default sits where C++ `-O3` (without fast-math)
  sits.
- **Rust on stable can't reach Perry `--fast-math` on `loop_overhead`**
  because there's no way to expose LLVM's `reassoc` flag on
  individual fadd instructions without nightly's `fadd_fast`
  intrinsic. With manual i64 accumulator + iterator form: 99 → 24
  ms (still 2× off Perry `--fast`). Rust's stable position is
  comparable to **Perry default** at 95-98 ms; the takeaway is
  that Perry default is in the same boat as Rust stable here.
- **Go has no `-ffast-math` flag and can't enable LLVM's reassoc
  pipeline**; on the optimization-probe kernels in this section,
  Go can't recover Perry-`--fast-math`'s lead. (Go does win on
  `loop_data_dependent` via FMA fusion — see TL;DR — so this
  limitation is workload-specific.)
- **Swift `-O -wmo` closes 71-75% of the gap to Perry `--fast`** on
  `loop_overhead` / `math_intensive` / `accumulate`.

### What each microbench actually measures

[`METHODOLOGY.md`](polyglot/METHODOLOGY.md) — full
benchmark-by-benchmark explanation: what's in the inner loop, what
LLVM does with it, what each language's compiler does differently,
why the cell is the number it is. Read this if you suspect any cell
of being unfair.

---

## 3. Memory + GC stability

[`scripts/run_memory_stability_tests.sh`](../scripts/run_memory_stability_tests.sh)
+ [`test-files/test_memory_*.ts`](../test-files/) +
[`test-files/test_gc_*.ts`](../test-files/) — 6 tests × 3 GC mode
combos (default / mark-sweep escape hatch / gen-gc + write
barriers) = 18 runs per CI invocation.

### What each test catches

All numbers from the most recent run on this commit (M1 Max, macOS
26.4). The test asserts RSS stays under the per-test ceiling; the
"Current" column is the actual measured peak.

| Test | What it catches | RSS limit | default | mark-sweep | gen-gc+wb |
|---|---|---:|---:|---:|---:|
| `test_memory_long_lived_loop.ts` | Block-pinning, PARSE_KEY_CACHE leak, tenuring-trap regressions | 100 MB | 54 MB | 54 MB | 54 MB |
| `test_memory_json_churn.ts` | Sparse-cache leak, materialized-tree retention, tape-buffer leak | 200 MB | 91 MB | 91 MB | 91 MB |
| `test_memory_string_churn.ts` | SSO-fast-path-miss alloc, heap-string GC loss | 100 MB | 48 MB | 48 MB | 48 MB |
| `test_memory_closure_churn.ts` | Box leak, closure-env retention, shadow-stack slot leak | 50 MB | 13 MB | 13 MB | 13 MB |
| `test_gc_aggressive_forced.ts` | Conservative-scanner misses, parse-suppressed interleaving, write-barrier mid-mutation | 50 MB | 9 MB | 9 MB | 9 MB |
| `test_gc_deep_recursion.ts` | Stack-scan correctness during deep recursion | 30 MB | 6 MB | 6 MB | 6 MB |

All 18 cells (6 tests × 3 modes) PASS on this commit.

`test_memory_json_churn` dropped from 115 MB → **91 MB** when the
generational-GC default flipped to ON in v0.5.237 (-21%).

### bench_json_roundtrip RSS history

Direct path (`PERRY_JSON_TAPE=0`, 50 iterations of 10k-record parse +
stringify, peak RSS via `/usr/bin/time -l`).

> **Methodology note**: rows v0.5.193..v0.5.241 used best-of-5 minimum
> (the methodology in use when those releases shipped). The
> v0.5.279 row is RUNS=11 median + worst-observed peak RSS, the same
> methodology TL;DR §A and §B use today. The "Time (ms)" gap between
> the v0.5.241 row's 375 ms (best-of-5 min) and the v0.5.279 row's
> 382 ms (RUNS=11 median) is the noise floor that motivated the
> methodology change — not a regression. RSS is unchanged because
> peak occupancy is set by GC trigger geometry, not by aggregation
> method.

| Version | RSS (MB) | Time (ms) | Change |
|---|---:|---:|---|
| pre-tier-1 (v0.5.193) | ~213 | ~322 | baseline |
| v0.5.198 (threshold 64 MB) | 144 | 364 | tuned initial threshold |
| v0.5.231 (C4b-γ-1, evac no-op) | 109 | ~80 | block-persist + tenuring + arena fixes |
| v0.5.234 (C4b-γ-2, evac live) | 142 | 358 | rebuilt baseline (post-other-changes) |
| v0.5.235 (C4b-δ, dealloc) | 142 | 358 | dealloc fires but peak is pre-first-GC |
| v0.5.236 (C4b-δ-tune, ceiling) | 107 | 358 | trigger ceiling stops step doubling past 64 MB |
| v0.5.237 (gen-gc default ON) | 102 | 372 | minor GC fires by default |
| v0.5.241 (best-of-5 min) | 102 | 375 | unchanged from v0.5.237; last best-of-5 row |
| **v0.5.279 (current, RUNS=11 median)** | **102** | **382** | RUNS=11 median (p95=389, σ=3.9, [377..389]) |

Default (lazy + gen-gc), the case `bench_json_roundtrip` measures with
no env vars: **70 ms median / 85 MB peak RSS** (RUNS=11; p95=73, σ=1.1,
[69..73]). On this specific workload (parse + stringify, no
intermediate iteration), faster than every other TypeScript-input
runtime measured here (Node, Bun); slower than simdjson (C++ + SIMD,
the parse-throughput ceiling). See TL;DR §A for the full table and
the workload caveats — the lazy tape's win is workload-specific, and
this is the workload it was designed for.

The 70 ms here matches TL;DR §A's `perry (gen-gc + lazy tape)` row
within run-to-run noise (75 ms median in §A's 11-run sample, σ=6.9).
Both numbers come from the same workload on the same v0.5.279 binary;
the 5 ms gap is the difference between two independent RUNS=11
samples, and is well inside the σ envelope.

### Other Perry benches (RUNS=11, M1 Max, taskpolicy -t 0 -l 0)

Median + p95 + σ + min + max wall-clock ms, worst-observed peak RSS —
the same methodology used by TL;DR §A and §B. Refreshed 2026-04-25
at v0.5.279 using the same `compute_stats` awk routine as
[`json_polyglot/run.sh`](json_polyglot/run.sh) over each suite binary
in [`benchmarks/suite/`](suite/). All rows here measure JSON, GC, or
integer/cache work — none are FP-arithmetic-sensitive, so the
v0.5.585 fast-math opt-in flip doesn't move them; rerunning is on
the followup list but the gap is below the noise floor for the
workloads listed.

| Benchmark | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---:|---:|---:|---:|---:|---:|
| `bench_json_roundtrip` (default, lazy + gen-gc) | 70 | 73 | 1.1 | 69 | 73 | 85 |
| `bench_json_roundtrip` (`PERRY_JSON_TAPE=0`) | 382 | 389 | 3.9 | 377 | 389 | 102 |
| `bench_json_roundtrip` (`PERRY_GEN_GC=0`) | 70 | 71 | 1.0 | 68 | 71 | 85 |
| `bench_json_roundtrip` (both opts off) | 358 | 360 | 2.0 | 354 | 360 | 102 |
| `bench_json_readonly` (default) | 66 | 68 | 1.0 | 65 | 68 | 81 |
| `bench_json_readonly` (`PERRY_JSON_TAPE=0`) | 291 | 309 | 5.7 | 286 | 309 | 104 |
| `07_object_create` | 0 | 1 | 0.4 | 0 | 1 | 6 |
| `12_binary_trees` | 1 | 1 | 0.5 | 0 | 1 | 6 |
| `bench_gc_pressure` | 17 | 21 | 1.1 | 17 | 21 | 25 |
| `04_array_read` | 5 | 9 | 1.7 | 4 | 9 | 211 [^arr] |
| `05_fibonacci` | 315 | 333 | 5.5 | 312 | 333 | 6 |
| `08_string_concat` | 0 | 1 | 0.5 | 0 | 1 | 6 |

[^arr]: Working set, not a leak — index-based fill (`arr[i] = i`) triggers
    doubling reallocation; the last grow temporarily holds both 8M-cap
    (64 MB) and 16M-cap (128 MB) buffers in the arena. Full math +
    `PERRY_GC_DIAG=1` trace in
    [`benchmarks/polyglot/ARRAY_READ_NOTES.md`](polyglot/ARRAY_READ_NOTES.md).

---

## 4. Strengths

Where Perry actually wins, and a one-line "why" per item.

- **JSON validate-and-roundtrip — best in dynamic-typing pack**
  (parse → stringify, no intermediate iteration). Perry lands at
  **75 ms** median (TL;DR §A) — faster than every other
  dynamic-typing runtime in the table: Bun 259 ms, Node 394 ms,
  Kotlin server JIT 453 ms. simdjson leads
  the absolute time at 24 ms — that's the SIMD-accelerated C++
  reference, listed alongside nlohmann/json so the comparison is
  honest in both directions. Perry's win in the dynamic-typing
  cohort comes from the lazy JSON tape (v0.5.204+): parse builds
  a 12-byte-per-value tape instead of materializing a tree;
  stringify on an unmutated parse memcpy's the original blob —
  same fast-path trick simdjson uses with `raw_json()`. See
  [`json-typed-parse-plan.md`](../docs/json-typed-parse-plan.md).
  On parse-and-iterate (TL;DR §B), Perry doesn't lead — simdjson
  at 24 ms and Rust serde_json at 183 ms both beat Perry's 466 ms,
  and Perry's lazy tape pays overhead it can't amortize when every
  element is touched.
- **Release-mode defaults expose LLVM optimizations that strict-IEEE
  languages need explicit flags to enable.** Perry emits f64
  arithmetic with `reassoc contract` fast-math flags — the minimum
  IEEE deviations TypeScript's `number` type can't observe (no
  signalling NaNs, no fenv, no operator-level `-0` strictness) — so
  LLVM's IndVarSimplify rewrites trivially-foldable accumulators as
  integer induction variables and the autovectorizer generates
  `<2 x double>` parallel-accumulator reductions. Rust / C++ / Swift
  / Go default to IEEE-strict and need `-ffast-math` /
  `-ffp-contract=fast` / nightly's `fadd_fast` to enable the same
  pipeline. On
  [`loop_data_dependent`](polyglot/bench.rs#L122) — the
  genuinely-non-foldable f64 kernel where the compiler *can't* fold
  the loop body away — Perry lands at **235 ms median**, dead in
  the no-contract compiled-pack cluster (Rust 229, Swift 233,
  Java 229, Bun 232; the FMA-contract pack of Go 128 / C++ `-O3`
  Apple Clang 129 wins this kernel by fusing FMUL+FADD into FMADDD,
  which LLVM matches under `-ffp-contract=fast`). The larger gaps
  Perry shows on `loop_overhead` / `math_intensive` / `accumulate`
  are *because those kernels are foldable* and Perry's defaults let
  the optimizer fold them; `clang++ -O3 -ffast-math` closes those
  gaps to within a millisecond
  (see [`RESULTS_OPT.md`](polyglot/RESULTS_OPT.md)). Those probe
  cells live in the TL;DR's "Optimization probes" subsection
  above — they measure compiler flag posture, not runtime
  performance, so they aren't on this list.
- **Object allocation in tight loops** (`object_create`, 1M iters) —
  ties native (0 ms). Working set fits in one arena block; GC never
  fires; the inline bump allocator is ~5 instructions per `new`.
- **Generational GC defaults that adapt** (`test_memory_json_churn`
  dropped 115 → 91 MB just from flipping the default) — the
  Bartlett-style mostly-copying generational implementation
  (v0.5.234-237) catches sustained-allocation workloads that pure
  mark-sweep handles poorly.

---

## 5. Weaknesses

The ones we already know about and what's tracked:

- **RSS on dynamic-JSON workloads is high vs typed-struct
  languages.** 85 MB vs Rust's 11 MB on the bench above. Fundamental
  to dynamic typing — every JSON value is a heap NaN-boxed object.
  Mitigation in flight: typed JSON parse (`JSON.parse<T>(blob)`) lets
  the compiler emit packed-keys pre-resolution.
  Step 1 done in v0.5.200.
- **GC pause is stop-the-world.** No concurrent marking. On
  `bench_gc_pressure`, this is 1-2 ms per cycle. On a multi-GB heap
  it would be much more. Tracked as a follow-up in
  [`generational-gc-plan.md`](../docs/generational-gc-plan.md)'s
  "Other parked items" section.
- **No old-generation compaction.** V8, JSC, HotSpot all compact
  old-gen; Perry doesn't. Fragmentation eventually accumulates;
  tracked as a follow-up.
- **Shadow stack is opt-in for the tracer's precision win.** The
  conservative C-stack scan still runs unconditionally because
  shrinking it requires platform-specific FP-chain walking; deferred
  with rationale in
  [`generational-gc-plan.md`](../docs/generational-gc-plan.md)
  §"Deferred follow-ups".
- **TypeScript parity gaps.** 28-test gap-test suite, 18 currently
  passing. Known categorical gaps (lookbehind regex, `console.dir`
  formatting, lone surrogate handling) tracked at
  [`typescript-parity-gaps.md`](../docs/typescript-parity-gaps.md).
- **No JIT.** Compiled code is fixed at build time. JS-engine JIT
  warmup gives V8/JSC a long-tail advantage on iteration-heavy code
  that Perry can't match.
- **Single-threaded by default.** `perry/thread` provides
  parallelMap / spawn but values cross threads via deep-copy
  serialization (no SharedArrayBuffer). Real shared-memory threading
  is not implemented.
- **No incremental / concurrent compilation.** Build time is
  monolithic; incremental rebuilds in v0.5.143's `perry dev` watch
  mode help but full compiles are not yet incremental.

---

## 6. Reproducing

### JSON polyglot

```bash
# In repo root, build Perry:
cargo build --release -p perry-runtime -p perry-stdlib -p perry

# Install the C++ JSON dependency (macOS):
brew install nlohmann-json

# Run the polyglot suite:
cd benchmarks/json_polyglot
./run.sh             # RUNS=11 default (median + p95 + σ + min + max)
RUNS=21 ./run.sh     # 21 runs for tighter intervals
```

Outputs `benchmarks/json_polyglot/RESULTS.md` with the full table.

### Compute microbenches

```bash
cd benchmarks/polyglot
./run_all.sh         # RUNS=11 default (median + p95 + σ + min + max)
./run_all.sh 21      # 21 runs for tighter intervals
```

Missing language toolchains show as `-` in the table; the script
degrades gracefully.

### Memory stability tests

```bash
bash scripts/run_memory_stability_tests.sh
```

Runs 18 test combinations (6 tests × 3 GC modes), prints PASS/FAIL +
RSS per cell. Wired into CI via `.github/workflows/test.yml`.

---

## 7. Design / implementation references

- [`docs/generational-gc-plan.md`](../docs/generational-gc-plan.md) —
  the GC architecture: phases A-D, write barriers, evacuation,
  conservative pinning, plus the academic + industry lineage
  appendix (Bartlett 1988, Ungar 1984, Cheney 1970, etc.).
- [`docs/json-typed-parse-plan.md`](../docs/json-typed-parse-plan.md) —
  the JSON pipeline design: tape format, lazy materialization,
  typed-parse plan.
- [`docs/audit-lazy-json.md`](../docs/audit-lazy-json.md) — external
  reviewer reference for the lazy-parse correctness guarantees +
  access-pattern matrix.
- [`docs/memory-perf-roadmap.md`](../docs/memory-perf-roadmap.md) —
  RSS optimization roadmap (tier 1: NaN-boxing, tier 2: SSO, tier 3:
  generational GC).
- [`docs/sso-migration-plan.md`](../docs/sso-migration-plan.md) —
  Small String Optimization rollout sequencing.
- [`benchmarks/polyglot/METHODOLOGY.md`](polyglot/METHODOLOGY.md) —
  per-microbenchmark explanation, compiler versions, why each cell
  is the number it is.
- [`CHANGELOG.md`](../CHANGELOG.md) — every version, every change,
  with measured impact where applicable.

If you spot something that looks unfair, biased, or wrong: open an
issue at https://github.com/PerryTS/perry/issues with the
benchmark name, your alternative implementation, and the toolchain
versions you ran with. The point of this page is to be defensible,
not to win. Numbers that don't survive scrutiny don't belong here.

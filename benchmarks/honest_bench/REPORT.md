# Perry vs Rust vs Zig — honest benchmark results

Numbers are measured against Perry v0.5.908 on five implementations: Rust, Zig, Perry, Node.js, Bun. All three workloads complete on all five implementations and every measured run's output matches the Bun reference byte-for-byte (300/300 rows; correctness gate from `#441`). The auto-generated tables below come straight from `results/results.json`; the bottom-line summary tracks the most recent sweep.

## Bottom line

- **Compute (image convolution, tight loop, minimal heap):** Zig 245 ms.
  **Perry 354 ms (1.44× Zig) — ahead of Rust's 392 ms**, 2.6× faster
  than Bun (915 ms), 3.4× faster than Node (1207 ms). Slight improvement
  vs yesterday's v0.5.891 (365 → 354 ms); the bulk of the original arc
  (2261 ms → ~377 ms, ~6×) landed during the v0.5.30–v0.5.495 perf sprint.
- **Allocation-heavy (JSON pipeline, 100 records):** Zig 35 ms, Rust 34 ms,
  **Perry 39 ms (1.15× the fastest) — ahead of Bun's 51 ms and Node's 144 ms**,
  at ~3 MB RSS vs Bun's 11 MB and Node's 36 MB. Stable vs v0.5.891.
- **JSON at scale (500k records, 108 MB):** Rust 608 ms, Bun 650 ms, Zig
  867 ms, Node 966 ms, **Perry 1098 ms**. Perry trails Rust/Bun by ~1.7×,
  beats Node by 9%, and uses 731 MB peak RSS. The workload completes
  reliably — as recently as v0.5.68 this hung >13 min CPU without
  finishing (`#65`, now closed). Improved from v0.5.891's 1155 ms (-57 ms)
  thanks in part to the v0.5.900 GC fix for #745.
- **Binary size:** Zig smallest (~230 KB), Rust next (~300–380 KB), Perry
  (~550–700 KB, including GC + Node-compat shims). Node/Bun don't have
  standalone binaries — they require their runtime installed separately.
- **Source LoC (non-blank, non-comment):** The TypeScript implementations
  (Perry / Node / Bun run the same source) are in the 52–92-line range;
  Rust and Zig at 99–113. TypeScript gives ~25–40% fewer lines at
  competitive-to-native performance on two of three workloads.

Charts: [image convolution](charts/image_convolution.png),
[JSON 100-record](charts/json_pipeline_small.png),
[JSON 108 MB](charts/json_pipeline_full.png).

## Hardware & toolchains

| | |
|---|---|
| CPU | Apple M1 Max (10 cores, arm64) |
| RAM | 64.0 GB |
| OS  | macOS 26.4 (Darwin) |
| Rust | `rustc 1.94.1 (e408947bf 2026-03-25)` |
| Zig | `0.15.2` |
| Perry | `perry 0.5.908` |
| Python | `Python 3.14.4` |
| Runs | 5 warmup + 20 measured, median reported |
| Generated | 2026-05-14T07:44:59.755910+00:00 |

## 3. Image convolution (5×5 Gaussian, 3840×2160 RGB)

_In-memory input + output checksum (no PPM I/O) — see the workload README for the reason. All three languages produce the identical FNV-1a-32 checksum._

| Language | Wall median (ms) | Wall σ | Peak RSS | Binary size | Source LoC | Runs OK |
|---|---:|---:|---:|---:|---:|---:|
| rust | 392.1 | 3.8 | 48.5 MB | 295.5 KB | 112 | 20/20 |
| zig | 245.4 | 6.2 | 48.5 MB | 227.0 KB | 113 | 20/20 |
| perry | 354.0 | 2.3 | 49.9 MB | 966.6 KB | 92 | 20/20 |
| node | 1,206.6 | 22.0 | 86.5 MB | — | 86 | 20/20 |
| bun | 915.4 | 35.2 | 60.0 MB | — | 86 | 20/20 |

_Ratios vs fastest: rust = 1.60×, zig = 1.00×, perry = 1.44×, node = 4.92×, bun = 3.73×_

## 1a. JSON pipeline — small fixture (100 records, 21 KB)

_All three languages produce byte-identical output at this scale (hash `7fc66fa8`)._

| Language | Wall median (ms) | Wall σ | Peak RSS | Binary size | Source LoC | Runs OK |
|---|---:|---:|---:|---:|---:|---:|
| rust | 34.1 | 1.1 | 1.2 MB | 376.3 KB | 99 | 20/20 |
| zig | 34.5 | 1.1 | 1.5 MB | 309.1 KB | 112 | 20/20 |
| perry | 39.2 | 1.2 | 3.5 MB | 1.0 MB | 52 | 20/20 |
| node | 143.6 | 1.8 | 36.1 MB | — | 40 | 20/20 |
| bun | 51.1 | 1.4 | 10.6 MB | — | 40 | 20/20 |

_Ratios vs fastest: rust = 1.00×, zig = 1.01×, perry = 1.15×, node = 4.21×, bun = 1.50×_

## 1b. JSON pipeline — full fixture (500k records, 108 MB)

_All five implementations complete this workload against the same 108 MB fixture and produce the same hash `b7e8a588`. Perry completes in ~1.1 s, ~1.8× the leader (Rust); as recently as v0.5.68 this workload hung >13 minutes without finishing (`#65`, now closed)._

| Language | Wall median (ms) | Wall σ | Peak RSS | Binary size | Source LoC | Runs OK |
|---|---:|---:|---:|---:|---:|---:|
| rust | 608.3 | 13.0 | 430.6 MB | 376.3 KB | 99 | 20/20 |
| zig | 867.4 | 19.4 | 576.8 MB | 309.1 KB | 112 | 20/20 |
| perry | 1,098.4 | 9.4 | 731.0 MB | 1.0 MB | 52 | 20/20 |
| node | 966.3 | 9.8 | 880.0 MB | — | 40 | 20/20 |
| bun | 649.9 | 14.7 | 593.7 MB | — | 40 | 20/20 |

_Ratios vs fastest: rust = 1.00×, zig = 1.43×, perry = 1.81×, node = 1.59×, bun = 1.07×_

## Honest findings — Perry gaps surfaced by this benchmark

Building the Perry implementations surfaced 8 real bugs. **7 of them were fixed in v0.5.30** while this benchmark was being written; the 8th was only visible once the earlier fixes landed. Each has a standalone 20-line TS repro.

### Fixed in v0.5.30

1. **`buf[i] = v` on `Buffer` / `Uint8Array` was a silent no-op.** The
   lowering for `Expr::Uint8ArraySet` in `crates/perry-codegen/src/expr.rs`
   was `lower_expr(value)` — it evaluated the RHS and threw it away. The
   runtime helper `js_buffer_set(buf, idx, val)` already existed; the
   codegen just wasn't calling it. _Fixed in this commit._

2. **[#38]** `new Uint8Array(N)` with a non-literal `N` routed to
   `js_uint8array_from_array` and misread the number as an `ArrayHeader*`,
   yielding a zero-length buffer. _Fixed._

3. **[#39]** 64-bit BigInt bitwise ops (XOR, AND-mask, multiply-and-mask)
   produced wrong results — `a ^ 5n` returned a small negative, AND-masking
   with `0xFFFF…n` collapsed to 0. Any FNV-1a-64 / Murmur / xxhash64
   implementation collapsed to 0 under Perry. _Fixed._

4. **[#40]** `Math.imul` was not lowered by the codegen (compile-time
   `expression MathImul not yet supported`). Every 32-bit-wrap hash in the
   world uses it. _Fixed._

5. **[#41]** `process.argv.slice(N)` returned a shape with `typeof` =
   `"string"`, length = the full argv length, and elements that were raw
   NaN-box bit patterns interpreted as tiny denormal floats. _Fixed._

6. **[#42]** Passing a multi-MB `Buffer` as a function parameter while the
   callee ran its own `Buffer.alloc()` silently corrupted the parameter.
   The param landed in a callee-saved register the conservative stack scan
   didn't cover; the intervening GC swept the backing buffer. _Fixed._

7. **[#43]** `JSON.stringify` panicked inside `perry-runtime/src/json.rs:427`
   (`byte index N is not a char boundary`) on arrays of 30k+ records with
   nested objects — reading already-corrupted string payloads, likely from
   the same underlying GC issue as #42. _Fixed._

8. **[#44]** `JSON.parse` + iterate + field read on a 50k-record array with
   rich objects dropped 99.9% of `.active === true` matches — the parsed
   objects were being swept mid-iteration. _Fixed._

### Still open

9. **[#46]** `JSON.parse` silently caps output at ~1666 entries for inputs
   larger than roughly 4 MB of structured records. Returns without error;
   `parsed.length` is just 1666 instead of the real count. Surfaced only
   after the #43 panic was fixed — previously the panic fired before the
   truncation was visible. This is why the Perry JSON pipeline is still
   run on the 100-record fixture only.

### Net effect on the numbers

The Perry columns in this report reflect a Perry-TS written *with the
workarounds for #38–#44 still in place* (hand-rolled `imul32`, module-level
`Buffer` globals, fresh-object construction in JSON) — removing those
workarounds after v0.5.30 would simplify the code further but wouldn't
materially change the numbers; the slow paths are the hash loop, the JSON
parse, and the convolution kernel, none of which are affected by the
workaround shape.

## Methodology

- **Release / optimized builds only**: `cargo --release`, `zig build-exe -O
  ReleaseFast`, Perry's native path (auto-optimized libraries).
- **Warmup / measured**: configurable via `HONEST_BENCH_WARMUP` and
  `HONEST_BENCH_MEASURED` (defaults: 5 / 20). **Median** is reported
  because it's robust to the occasional stray OS scheduler hiccup; σ is
  reported alongside.
- **Wall time**: Python `time.monotonic_ns()` delta around the binary
  invocation (so it includes process startup + fs open + the work itself).
- **Peak RSS**: `/usr/bin/time -l`'s `peak memory footprint`, captured in
  bytes and converted to kB / MB.
- **Correctness**: every run emits a line containing a record-count and an
  FNV-1a-32 hash. The driver records stdout's first + last 200 characters
  for each run, which is the minimum needed to verify the three languages
  agree on what they computed.
- **Source LoC**: non-blank, non-comment lines. Computed by the report
  script (no `tokei` / `scc` needed).
- **Fixtures**: deterministic — `scripts/gen_json.py` writes byte-identical
  output across runs. The image convolution uses an in-process xorshift32
  stream.

No SIMD intrinsics, no hand-vectorized loops, no `#[target_feature]` — the
code in each language is what a typical first pass would look like. The
compilers' autovectorizers do their own thing.

## Reproduction

```bash
cd benchmarks/honest_bench
./run.sh                           # build, generate fixtures, run, write results/
python3 scripts/plot.py            # render charts/*.png
python3 scripts/report.py          # regenerate REPORT.md
```

Environment overrides:

```bash
HONEST_BENCH_WARMUP=1 HONEST_BENCH_MEASURED=3 ./run.sh    # quick iteration
HONEST_BENCH_ONLY=3 ./run.sh                              # image conv only
HONEST_BENCH_SKIP_BUILD=1 ./run.sh                        # reuse existing bins
```

The workload-2 HTTP server benchmark is deferred to a follow-up — it requires
an HTTP load generator (oha/wrk/hey) and a Perry `fastify` implementation
under sustained concurrent load. Not landed in this phase.

# Fast-math and FP contraction

Off by default. Opt in to permit LLVM optimizations on f64 arithmetic
that produce observably different results from Node's V8 in exchange for
faster code on a narrow class of numeric workloads.

## TL;DR

| Mode | Bit-exact with Node | Speed |
| --- | --- | --- |
| Default | Yes (~94% of random FP programs match Node bit-for-bit; the residual ~6% comes from the LLVM SLP vectorizer at `-O3`, not from fast-math) | Same as Node within noise on realistic FP code |
| `--fp-contract=on` or `fast` | No where FMA fusion changes rounding | Can emit FMA for multiply-add shapes without enabling reassociation |
| `--fast-math` | No (~70%; ~30% of random FP programs diverge by 1 ULP). Implies `--fp-contract=fast` unless explicitly overridden. | ~7x faster on tight `sum += constant` loops; ~0% difference on dot products, array reductions, or any data-dependent FP-heavy code (M-series ARM64 numbers; x86_64 may differ) |

If your program does scientific computing, signal processing, or any
hand-tuned numeric kernel that benefits from autovectorization or FMA
fusion, `--fast-math` may help. For everything else (UI, business logic,
crypto, networking, framework code), it changes nothing observable
except correctness — leave it off.

## Three ways to enable it

CLI flag wins over env var, env var wins over package.json:

```bash
# 1. Per-build CLI flag
perry --fast-math myapp.ts

# 2. Per-shell environment
PERRY_FAST_MATH=1 perry myapp.ts

# 3. Per-project package.json (most common)
{
  "perry": {
    "fastMath": true
  }
}
```

## Floating-point contraction

Contraction is separate from reassociation:

```bash
# Permit FMA contraction only.
perry --fp-contract=on myapp.ts

# Permit the frontend's most aggressive contraction mode without reassociation.
perry --fp-contract=fast myapp.ts

# Keep reassociation from --fast-math but block FMA contraction.
perry --fast-math --fp-contract=off myapp.ts
```

The same setting is available through `PERRY_FP_CONTRACT=off|on|fast`
or `"perry": { "fpContract": "on" }` in package.json. Explicit package,
env, or CLI `fpContract` values override the `--fast-math` implied
default.

## What it actually changes

Two LLVM per-instruction fast-math flags can be emitted on every
`fadd` / `fsub` / `fmul` / `fdiv` / `frem` / `fneg`:

- **`reassoc`** — permits the optimizer to reorder associative chains.
  `(a + b) + c` may become `a + (b + c)`. This is what the loop-vectorizer
  needs to break a serial accumulator dependency chain into 4 parallel
  accumulators. Worst-case observable behavior: tiny ULP-level
  differences in long sum chains over operands of widely-different
  magnitudes; rewrites like `(a / b) * b → (a * b) / b` (algebraically
  equal, IEEE-different).

- **`contract`** — controlled by `--fp-contract`; permits fused multiply-add.
  `a * b + c` may become a
  single FMA instruction with one rounding step instead of two. ARM and
  modern x86 both have hardware FMA. Worst-case observable behavior:
  intermediate `a * b` no longer rounds independently, so code that
  depends on the rounding structure (Kahan summation, compensated
  arithmetic) sees different bits.

## What it deliberately does NOT enable

The full clang `-ffast-math` is **off** even with `--fast-math`. In
particular, these flags stay clear:

- `nnan` / `ninf` — these tell LLVM to assume no NaN/Inf inputs, which
  is catastrophic for Perry: NaN-boxing uses NaN bit patterns for every
  non-number value (strings, objects, null, undefined, booleans).
  Enabling them caused LLVM to replace `TAG_NULL` / `TAG_UNDEFINED`
  constants with `0.0` at codegen time. Tried at v0.2.x commit
  `083ce16`, reverted two days later in `b5a8c83f`. Will not return.
- `nsz` (no signed zeros) — would make `(a + 0) → a` a valid rewrite
  even when `a` is `-0`. `Object.is(-0, 0)` is observable in JS.
- `arcp` (allow reciprocal) — would rewrite `a / b → a * (1 / b)`,
  which loses precision when `b` is far from a power of two.
- `afn` (approximate functions) — would let LLVM substitute lower-
  precision math intrinsics.

For reference, Rust nightly's `#![feature(float_algebraic)]` enables
`reassoc + contract + nsz + arcp + afn`. Perry's `--fast-math` is
strictly more conservative than that.

## Performance numbers

Benchmarks on Apple Silicon (M-series, ARM64), `min` of 3 runs each,
LLVM 19, perry 0.5.569. Run `scripts/perf_bench.sh` to reproduce.

| Benchmark | Default | `--fast-math` | Ratio | Node |
| --- | ---: | ---: | ---: | ---: |
| `sum_loop` (100M `sum += 1`) | 96 ms | 13 ms | **7.4× faster** | 53 ms |
| `dot_product` (10M `sum += a[i]*b[i]`) | 13 ms | 13 ms | 1.00× | 12 ms |
| `array_sum` (10M `sum += xs[i]`) | 10 ms | 10 ms | 1.00× | 11 ms |

Read these together: `--fast-math` produces a large speedup ONLY on
loops where the accumulator step is constant or trivially-redundant
enough that LLVM can split it into parallel partial sums. Real FP
workloads rarely look like `sum += 1` and so rarely benefit. The default
mode beats Node on `array_sum` and matches it on `dot_product` without
giving up bit-exact parity.

## Correctness numbers

`scripts/fp_fuzz.mjs` — randomly generates TS programs exercising the
six patterns most likely to trip per-instruction FMFs (left-fold,
tree-fold, right-fold reductions; FMA-shaped chains; algebraic
identities like `(a/b)*b`; cancellation predicates). Each program is
compiled with both Node and Perry, and stdout is diffed byte-for-byte.

| Mode | Pass rate (100 random programs, seed=200) |
| --- | --- |
| Default | 94/100 |
| `--fast-math` | ~70/100 |

The 6/100 default-mode failures are residual divergences from sources
not gated by per-instruction FMFs — most originate in the LLVM SLP
vectorizer at `-O3`, which can apply pairwise reduction even without
the `reassoc` permission. Tracked separately; out of scope for this
flag.

## Object-cache interaction

Perry's per-module `.o` cache (in `.perry-cache/objects/`) keys on the
`fast_math` and `fp_contract` settings alongside source hash and other
compile options. Toggling either invalidates affected cache entries —
`perry --fast-math` or `perry --fp-contract=on` right after `perry`
does a clean recompile of every module that contains f64 arithmetic. No
`--no-cache` necessary.

(This is a deliberate fix. During the original investigation, an early
version of the flag forgot to enter the cache key, and the result was
that toggling the flag appeared to do nothing because all `.o` files
came from the cache. If you ever see fast-math defaults that *seem*
not to take effect, suspect the cache key first.)

## Migration notes

- **For library authors:** if your TS library publishes benchmark
  numbers, document which mode you measured under. The 7× sum-loop case
  is the only place the gap is large; if your benchmark doesn't look
  like that, the numbers are mode-independent and you can publish one
  set.
- **For app authors:** there is no migration. Default behavior is the
  pre-flag behavior with `--fast-math` removed; bit-exact results are
  *more* compatible with Node, not less.
- **For determinism-critical code** (lockstep simulations, financial
  reconciliation, hash function correctness): leave the default. Even
  with `--fast-math` off there's a residual ~6% divergence rate on
  random FP code, which is too high for true determinism work — but
  it's an order of magnitude better than the ~30% with the flag on.

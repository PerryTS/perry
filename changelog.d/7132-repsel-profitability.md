### Fixed

- **repsel: canonical i32 is now chosen on benefit, not only on provability
  (#7128).** `benchmarks/suite/15_mandelbrot.ts` regressed **+14.87%
  instructions retired** at #7121, measured on a quiet Raspberry Pi 5 with
  `perf stat` at a 0.02% noise floor and bisected by binary hash. Wall time did
  not move (48 ms vs 49 ms) because the workload is FP-latency-bound, which is
  why nothing caught it.

  **Root cause, read out of the emitted AArch64 rather than inferred.** The
  innermost loop is `while (x*x + y*y <= 4.0 && iter < MAX_ITER)`. With `iter`
  a boxed double, both exit tests are FP and LLVM fuses them into `fcmp` +
  `fccmp`: **one basic block, 12 instructions, one branch**. #7122's monotone
  loop-induction interval proves `iter ∈ [0, 100]` and #7121 let that proof
  reach the module-init body, so `iter` took a canonical i32 slot — and an
  integer compare cannot fuse with an FP compare. The loop splits into **two
  blocks totalling 14 instructions**, plus a `ucvtf` where the accumulator
  joins. 2 instructions × 8,011,148 innermost iterations ≈ 16.0M, against a
  measured +15.63M. `px` and `py` are the same shape one level out.

  **The defect was not the proof.** The proof is correct. The Let-site
  eligibility gate in `stmt/let_stmt.rs` was a conjunction of "may we?" terms
  with no "should we?" term anywhere in it, so widening the proof
  automatically widened the emission.

  **The fix is a profitability model** (`collectors/repsel_benefit.rs`),
  consulted by the selection gate as one more conjunct. For an i32-range value
  a `double` is a lossless, equal-cost representation of `+`, `-` and
  comparison; canonical i32 only *buys* something where the consumer cannot
  take a double without a conversion — array/typed-array indexing, bitwise
  operands, `Math.imul` — and becomes a *cost* the moment a hot consumer needs
  the double back. So a local that is written after its declaration, has no
  i32-consuming read anywhere, and has at least one double-consuming read
  inside a loop stays boxed. The model only ever refuses, so every uncertainty
  resolves toward "not a cost" (comparison is neutral on both sides —
  `for (let i = 0; i < n; i++)` with a `number` parameter must keep
  promoting).

  Measured on the Pi, 11 repeats, `perf stat -e instructions:u`:

  | workload | before | after | Δ |
  |---|---|---|---|
  | `15_mandelbrot` | 120,738,701 | 105,110,087 | **−12.94%** |
  | `11_prime_sieve` (the #7121 win) | 2,597,182,143 | 2,597,177,676 | −0.00% |
  | `08_string_concat` (the #7121 `Str` win) | 30,281,798 | 30,281,711 | −0.00% |

  Both #7121 wins re-measured against the `at7122` arm with this compiler:
  canonical `Str` **−4.12%**, canonical i32 **−1.05%**. They hold by
  construction — the linked binary for each is byte-identical to `main`'s.

  Over the whole 26-workload census corpus the emitted object changes on
  **exactly one** program, and there its disassembly is byte-identical to the
  pre-#7121 compiler's. The other 9 refused promotions were already emitting
  byte-identical code, so the census counts fall (canonical-i32 64 → 55)
  without a single emitted byte moving.

  **Gated.** Every other number in the promotion census is a floor, and a floor
  cannot go red when a compiler promotes *more*. `REFUSAL_FLOORS` in
  `scripts/compiler_output_harness/repsel_census.py` gives the refusal its own
  minimum; `benchmarks/repsel_census/fixtures/fixture_loop_bounded_i32.ts` now
  carries `iterate()` and `mixedWithFloat()` side by side — same #7110 interval
  proof, opposite verdict, differing only in what consumes the counter — so
  neither an always-yes nor an always-no rule can satisfy the file.

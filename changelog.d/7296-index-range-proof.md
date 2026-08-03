### Fixed

- **Array indexing: prove ranges for strided counters and constant-argument parameters (#7286).**
  `numeric_index_needs_runtime_key` (`crates/perry-codegen/src/expr/index_get.rs`) demotes an
  array read *and* write to the fully opaque `js_array_get_index_or_string` /
  `js_typed_feedback_array_set_index_or_string` runtime-key helpers unless the index carries a
  `[0, i32::MAX]` range proof — no inline header test, no inline load, nothing LLVM can see
  through. Two ubiquitous shapes could not produce one:

  * **Strided induction counters.** Only `i++` with an integer-literal start was classified, so
    `for (let j = i * i; j < LIMIT; j = j + i)` — the whole inner loop of `11_prime_sieve`,
    ~2.12M stores in its timed region — was demoted.
  * **Numeric parameters.** A parameter arrives as a bare NaN-boxed `double` with no fact
    attached, and ranges compose through `+`/`*`, so one unbounded leaf poisons the whole index
    expression: `matmul(a, b, c, size: number)`'s `size` alone sank all three arrays in
    `16_matrix_multiply` (4 opaque calls per innermost iteration × 256³ = 67.1M calls).

  New `crates/perry-codegen/src/stmt/counter_range.rs` proves a **monotone-induction range**: a
  counter whose only writer is the loop's own update slot and whose step never decreases it is
  bounded above by the guard (re-evaluated at every body entry) and below by its initial value.
  It admits `j = j + <stride>` alongside `i++`, requiring a non-negative *integral* start, a
  non-negative *integral* stride, and the stride's operands loop-invariant and not
  closure-captured. It also now refuses a fact when the loop **body** writes the counter — a
  hole the old `i++` path had, since `for (let i = 0; i < 10; i++) { a[i]; i = -5; }` re-enters
  the body with `i === -4` while the fact still claimed `[0, 9]`.

  New `crates/perry-codegen/src/collectors/param_ranges.rs` derives **interprocedural range
  summaries** for numeric parameters: a meet of the argument constants over *every* call site
  of a function whose entire call graph is visible in the module. Any unresolved reference
  poisons it — exported, reflected onto `globalThis`, a `FuncRef` used as a value, an
  `Expr::Closure` sharing the id, an arity mismatch, a rest/default/`arguments` parameter, a
  parameter written or rebound anywhere at any depth, a duplicated `FuncId`, or one
  non-constant argument. The summary feeds `int_range_for_local` as a last resort, which is
  what lets the *existing* affine `a * b + c` composition in `int_range_expr` finally fire:
  `i * size + k` proves `[0, 65535]` once `size` is pinned to 256.

  Measured on an M1 Max (release build both arms, identical runtime archives,
  `PERRY_NO_AUTO_OPTIMIZE=1`), with byte-identical checksums: `16_matrix_multiply`
  **693 → 70 ms (9.9×)** and `11_prime_sieve` **118 → 31 ms (3.8×)**, taking
  `16_matrix_multiply` from 17.3× behind Node to 1.8×. `15_mandelbrot` (22 ms) and
  `05_fibonacci` (407 ms) are unchanged,
  and all 30 suite benchmarks produce identical output. In the IR, `matmul`'s innermost block
  goes from two `js_array_get_index_or_string` + two `js_number_coerce` calls to **zero `js_*`
  calls** — every remaining helper sits on a cold guard/fallback edge — and `prime_sieve`'s
  `sieve[j] = false` loses both the opaque store helper and the array-pointer re-anchoring
  that followed it.

  This is deliberately **not** about the index being an `i32`: `(i * size + k) | 0` *is* a
  genuine `i32` and buys nothing, because `ToInt32`'s `[-2^31, 2^31-1]` has `min < 0`. What
  was missing is non-negativity plus an upper bound. The `& 0x7fffffff` mask used to measure
  the prize in #7286 is a diagnostic device only and is not shipped — it changes semantics for
  negative and `>= 2^31` indices.

  Covered by `test-files/test_gap_array_index_range_proof.ts` (16 cases, byte-identical to Node
  26.5.1: negative / fractional / `NaN` / `-0` indices, `2^32-2` vs `2^32-1` vs `2^32`, indices
  above `i32::MAX`, an affine index that overflows `i32`, holey and sparse arrays, a body write
  to the counter, a body write to the stride, a callee that reassigns its parameter, a callee
  that escapes as a value, a non-integral stride, and an `+Infinity` start) plus 23 new
  `cargo-test`-visible `--lib` unit tests for the two admission sets.

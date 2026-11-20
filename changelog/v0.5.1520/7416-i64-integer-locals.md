**Fixed** JS `%` on an integer-valued local lowered to `frem`, which is not an
aarch64 instruction and becomes an `fmod` library call. `bench_bitwise` was
**20.4x slower than Node**; it is now **1.39x** — a 14.6x improvement, 55108ms to
3763ms, with the Node-verified `CHECKSUM:525000000` unchanged.

A guarded `srem` fast path already existed, but every gate in front of it asked
the wrong question. `is_integer_valued_expr` resolves a `LocalGet` through
`integer_locals`, which is an **i32-range** property — it also gates i32 shadow
slots, so widening it would have placed an i32-overflowing value into an i32 slot.
The `%` path converts to **i64** and only needs integer-valued-within-i64.

A new `collect_int_valued_i64_locals` supplies that weaker property as a
magnitude lattice, read only by the `%` gate. `integer_locals` is untouched.

Three holes were found and closed while building it, each of which would have
produced silently wrong arithmetic:

* **`Mul` blowup.** A pure integrality predicate let `(a*b*c) % n` reach
  `fptosi ... to i64` with a 93-bit product — poison. The lattice tracks
  magnitude and gates at 2^62. This also closes the same pre-existing hole for
  i32 locals.
* **Zero divisor.** An early version routed `1000 % d` into `srem` where `d`
  decrements through zero: `srem(x, 0)` is UB where JS requires NaN. The divisor
  is now restricted to a non-zero integer literal.
* **The saturation argument.** "±constant cannot leave i64 in finite time" is
  false for large deltas — `a = a + 1e18` escapes in ~10 iterations. Replaced
  with a hard IEEE-754 bound: once `ulp(v) >= 4D`, `v ± d` rounds back to `v`
  exactly, so `|L| <= 2^(55+log2 D)` forever. Deltas are capped at 64.

Also of note: the gate that actually decides this is
`expr/mod.rs::lower_numeric_binary_value`, which intercepts numeric binary ops
before `binary::lower` and only handed `Mod` off when the dividend had an i32
counter slot. There are six `frem` emission sites; widening the predicate alone
changed nothing, and the IR acceptance test caught that.

Verified: hot-function IR goes `frem 4 -> 0`, `srem 0 -> 4`; the edge-case
differential (including both `-0` cases, via `Object.is` rather than `===`) is
byte-identical to Node 26.5.1; `cargo test -p perry-codegen --lib` 632 passed;
26/27 math/number/int gap tests pass with the one failure pre-existing and
unrelated.

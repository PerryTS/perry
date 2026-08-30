Two numeric-lowering fixes for byte-array reads, found while working
`benchmarks/suite/bench_int_arithmetic.ts` (7.7× Node before this change; 6.2×
after — the rest is a separate index-shape gap, noted below).

**`acc += px[i]` on a `Uint8Array` no longer takes the dynamic add.** The HIR
lowers `px[i]` on a binding it already knows is a `Uint8Array`/`Buffer` to the
dedicated `Uint8ArrayGet` node, but the number-by-construction fixpoint's
number-or-`undefined` view-read rule — and the not-BigInt predicate beside it —
only recognised the `IndexGet` spelling. The accumulator was therefore never
admitted, and every `+=` lowered through `js_dynamic_string_or_number_add` with
`acc` kept in a GC-rooted shadow slot.

That fix has a correctness half. Once the add is a raw `fadd`, an out-of-bounds
read's NaN-boxed `undefined` survives it — IEEE arithmetic preserves the NaN
payload — and the accumulator reads back as `undefined` where the spec says
`NaN`. Number-context lowering now canonicalizes the value first, preferring the
guarded inline load (which sinks the NaN into its own out-of-bounds arm) and
otherwise emitting one compare plus a select, never a call. The `undefined` box
is the only non-double this node can produce, so the test is exact.

**Compound buffer indices can now prove their bounds.** `bounds_for_buffer_access_width`
proved only a single index local carrying a bounded-pair fact, or a constant, so
an index like `i * 2 + 1` or `y * 30 + x` fell back to a per-element
`js_uint8array_index_get_value` call. The interval analysis can bound those: every
leaf is a counter with a range fact or a compile-time constant, and `int_range_expr`
composes them with checked arithmetic, answering `None` as soon as a leaf is
unknown or a step overflows. When the whole interval fits inside a *constant*
buffer length, the access is in bounds on every iteration. A non-constant length
still declines — there is nothing to compare the interval against.

Measured on an idle Mac mini, min of three, self-timed: `bench_int_arithmetic`
475 → 395 ms (Node 64); a `pixels[y * SIZE + x]` variant 636 → 356 ms; a probe
whose reads all qualify drops from 54 per-element helper calls to none.

Verified byte-identical to Node on two differentials that are part of neither
suite: eight cases around the byte read itself (in-bounds accumulate, an
out-of-bounds read inside `+=`, an out-of-bounds read as a value, string
concatenation, negative and fractional indices, a `Buffer` receiver, a mixed
numeric/string accumulator) and seven around the new bounds proof (the exact
kernel shape, an interval touching the last element, a counter mutated inside
the loop body, a deliberately out-of-range interval, a one-past-the-end read, a
negative composite index, and a buffer whose length is not a compile-time
constant).

Left for a follow-up: `bench_int_arithmetic`'s own `(y + ky) * SIZE + (x + kx)`
still declines the interval proof, so its 54 reads per pixel remain calls.
Also worth knowing — `f64_kind_from_class` maps `"Uint8Array"` and
`"Uint8ClampedArray"` to a typed-array kind, but the checked load it feeds reads
the length at `handle + 0` and elements at `handle + 16`. Perry's
`new Uint8Array(n)` is buffer-backed (length at `data - 8`), so handing that path
one of those receivers makes every index compare out of bounds and silently
yields `undefined`. Nothing does today; the call site now says why.

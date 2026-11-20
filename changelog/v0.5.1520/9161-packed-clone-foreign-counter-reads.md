A packed loop's fast clone can now read its array at an **enclosing** loop's
counter, not only at its own.

The clone's element read is a raw slot load with no bounds check, licensed by the
index being the loop's own induction variable — which the loop bound already
proved in range. Any other index had no such proof, so `arr[i]` beside `arr[j]`
in a nested loop was rejected outright by the body matcher, and the whole inner
loop fell to the typed-feedback tier: a registered guard **call** plus a boxed
fallback per element, for both reads, on every iteration.

A foreign index now takes the same raw load behind one inline `icmp ult idx, len`
against the live `ArrayHeader.length` — the same word `expr/index.rs`'s store
guard reads — and branches to the fact's existing side exit when it fails. That
exit is the one the hole arm already uses, so mid-body exit needed no new
machinery.

**Read-only bodies only.** A side exit re-executes the iteration in the slow
clone, which is harmless for a read and would double-apply a store, so the
relaxation lives in `expr_is_packed_f64_loop_safe` and is deliberately not shared
with the store matchers beside it.

Measured on an idle Mac mini, self-timed, min of three: `benchmarks/suite/10_nested_loops.ts`
51 → 17 ms against Node's 16 — 3.3× Node to parity. `16_matrix_multiply` also
improves (121 → 100 ms) since its inner reads are the same shape. No other
benchmark in the 17-entry sweep moved.

Verified against Node on a five-case differential covering the shapes where a
side exit is observable: the plain nested accumulation, a labelled `break` out of
the inner loop mid-iteration, an `if` guarding the accumulation, a mixed-layout
array whose guard must fail into the slow clone, and an accumulator read by the
outer loop between inner passes. All byte-identical. `perry-codegen`: 30 suites
pass, including a new IR pin.

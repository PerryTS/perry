**An `arr.length`-bounded packed-f64 loop no longer loses its fast clone when the
body reads `a[k ± c]`** (#9259). It did not lose it partially — it lost it
entirely, taking the plain `a[k]` in the same loop with it: 70 ms → 36 ms on a
4096-element accumulate loop, and 60 ms → 19 ms on a comparison loop, against
node's 13 ms and 16 ms.

The cause was a cascade rather than a missed element load. Three separate
predicates encode "an index this loop's guard covers" as a bare
`Expr::LocalGet(counter_id)`, so `a[k - 1]` — an `Expr::Binary` — matched none of
them. The matcher's body walker declined, the offset read fell back to a helper
call, and the clone's own call-free scan then discarded the whole clone.

Two matchers each covered half the shape and neither covered the combination.
`lower_packed_f64_versioned_for` understands the `i < arr.length` bound but
publishes `window_validated: false`; `lower_packed_f64_range_versioned_for`
validates the offset window but accepts only a literal or loop-invariant bound,
and per its own call-site comment runs only after the first declined. Since
`arr.length` is the idiomatic spelling, the natural form was the slow one.

The fix admits a constant offset and pays the same inline `icmp ult idx, len` a
foreign counter already pays, taking the fact's existing side exit when it fails
— a compare and a never-taken branch, not a call, so the clone stays call-free.
The machinery existed already; what was missing was letting an offset index reach
it. Matcher and lowering now share one index parser, deliberately: a matcher that
admits what the lowering declines is not a missed optimisation, it is the same
9× regression arriving by another route.

Soundness rests on the guard being stronger than its flag name suggests. The
versioned guard ends in `js_array_is_numeric_f64_layout`, a whole-array property
that answers 0 for a holes-flagged array, so a passing guard means every
in-bounds slot is raw f64; `window_validated: false` is a statement about bounds,
not holes, and bounds are exactly what the inline check re-establishes. The
compare is unsigned, so a negative index (`a[k-1]` at `k == 0`) exceeds any
length and side-exits. Reads only — a store side exit re-executes the iteration,
harmless for a read and double-applying for a store.

Because the parser is shared, this also admits an offset on a foreign counter
(`a[j - 1]` for an enclosing loop's `j`), which is wider than the headline shape
and deliberate: the bounds check makes both cases identical.

`s += a[k] + a[k-1]` still pays a dynamic add — `accumulator_rhs_is_numeric` and
`has_numeric_index_fact` carry the same bare-`LocalGet` assumption — which is why
the comparison loop gains 3.2× and the accumulate loop 1.94×. That is being
addressed separately.

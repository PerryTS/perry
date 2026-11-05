**The affine index materialization can no longer wrap i64** (#9294
follow-up, from a review flag on its sibling PR).

#9294 computed `a[<affine>]` indices in i64 on the claim that proven-i32
leaves cannot overflow it. That is true for one multiply — `|i32 * i32|` is
at most 2^62 — and false beyond it: three chained near-2^31 factors reach
2^93, wrap i64, and a wrapped value that happens to land inside `[0, len)`
passes the unsigned bounds check and reads a DIFFERENT element than the
generic path, silently — JS computes the index in doubles, goes out of
bounds, and yields `undefined`.

Measured honestly: the wrap is LATENT today, not live. Neither a
const-folded spelling nor parameter leaves of a triple-multiply chain
currently reach the affine lowering — admission happens to be blocked by
which locals carry i32 shadow slots, an accident of unrelated analyses
rather than a guarantee. Widening shadow coverage is a plausible future
change, and it would have turned this into a silent wrong-read with no
failing test anywhere.

The fix is a static magnitude bound, `affine_index_magnitude_bound`:
interval arithmetic in i128 at match time with every leaf at its i32
extreme, admitting a tree only when its worst case fits i63. Admission
therefore costs nothing at run time; `i * size + k` (2^62 + 2^31) stays
admitted and matmul's numbers are unchanged, while any tree that could wrap
declines to the generic path. One shared predicate gates both the matcher
and the lowering, so the two cannot drift. A tripwire test pins the exact
2^64 tree (`2^21 * 2^22 * (2^21 + k)`) to node's `NaN` under both collector
modes — it passes today on both sides and exists to fail the moment
admission widens past the bound.

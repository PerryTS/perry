**Updated** `docs/engine-plan.md` with the resolved speed picture.

The earlier update recorded the first measurement; four corrections followed.
Both `bench_fibonacci` and `bench_bitwise` were measuring nothing, in two
different ways — a discarded result, and a loop-invariant call that hoists out
and runs once with the checksum still correct. Fixed and shipped in #7403, which
inverts the headline: `bench_bitwise` was reported as infinitely faster than Node
and is **20.4× slower**.

That gap is 4 `frem` per iteration lowering to `bl _fmod` — and the guarded
`srem` fast path **already exists** at `expr/binary.rs:588`, with the IEEE `-0`
correction. The gap is `is_integer_valued_expr`, reduced to a one-line repro:
reassigning the local (`a = a + 1`) disqualifies it. Four wrong hypotheses about
that subsystem are recorded in #7404 so they are not retried, along with the
reason a wrong widening is dangerous — silently wrong integer arithmetic that
`===` tests cannot see.

Also records a measured negative result: `js_array_grow`'s HOLE-init loop is
already vectorised (6 `stp` in the baseline), so the `slice::fill` rewrite
proposed for #7396 measures nothing.

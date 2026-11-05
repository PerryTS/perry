**A counted loop whose reads use an affine index now hoists its receiver guard
into the preheader** (#9253). `16_matrix_multiply` goes 100 ms → 69 ms against
node's 32 ms on an idle machine.

The interesting part was where the time was *not*. That loop spends 97% of its
time inside generated code with no runtime calls, so the cost was never a
missing inline or an unremoved helper. Per iteration, for both receivers, it
re-derived the pointer tag check, the handle-band check, the header
dereference, the `_reserved` flag tests and the two 16,000,000 length/capacity
sanity compares — for receivers that are loop-invariant parameters whose
headers cannot change inside the loop. LLVM cannot hoist that: the guard
reloads the header through a pointer it cannot prove unaliased, and the
incremental-barrier atomic read is a motion barrier.

The packed-f64 range tier already had everything required except a way to
describe `a[i * size + k]`, whose index has no compile-time window: it already
takes a loop-invariant local or parameter bound, emits one guard per receiver
AND-reduced into a single branch, and keeps its cached receivers GC-safe by
refreshing them on the back-edge poll. An access may now be *affine* — an
integer-producing expression over the loop counter and loop-invariant integer
locals — and such an access publishes a receiver-only fact: the entry guard
proves shape, raw-f64 packedness, integrity and the sanity bounds once in the
preheader, and each read pays one inline `icmp ult idx, len` with the fact's
existing side exit.

The index is materialised in i64 rather than i32, because `i * size` can exceed
i32 for a large matrix even when the final index is valid and an i32
computation would wrap — turning an out-of-bounds access into an in-bounds one.
The bounds compare is unsigned, so a negative index reads as a huge unsigned
value and side-exits; no static non-negativity proof is needed, which matters
because `size` is a parameter with no callsite range summary and no static
window for the product is obtainable. The index must also mention the counter:
a wholly loop-invariant index like `a[0]` is affine by the grammar but has a
compile-time window, and admitting it here made the classic walker succeed and
silently stole those loops from the dense tier's masked path that serves them
better.

Reads only, in the classic mode only. Dense mode's loads carry no side exit and
so cannot take a per-read bounds check, and an affine store is rejected because
the side exit re-executes the iteration.

This does not reach parity. The residual is the per-read bounds check and index
materialisation, plus the `c[i * size + j]` store in the enclosing loop, which
stays generic for the reason above.

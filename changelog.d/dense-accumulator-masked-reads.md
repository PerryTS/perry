**A float accumulator over masked reads now earns the dense range clone**
(`17_loop_data_dependent`: 475 ms → 219 ms against node's 220 ms on an idle
machine — parity, from 2.16×).

`sum = sum * x[i & 63] + x[(i * 7) & 63]` was rejected by the dense range tier
while `sum = sum * x[i & 63]` was admitted. The discriminator was the
accumulator's static numeric proof: `+` can be concatenation, so the
per-statement proof demands both operands numeric, and a reassigned
accumulator has no such proof — its own writes read the guarded array, whose
element proof only exists once the guard has run. A chicken-and-egg that `*`
never faces, because multiplication needs only the weaker inert fact.

The matcher now peels the accumulator: when the proof fails on the `LocalSet`
target of a self-accumulating write, it retries with the target treated as
numeric BY CONTRACT, records it pending, and then verifies every pending
local with the same collector the lowering runs — rejecting the whole dense
match (with its own named trace reasons) if the two disagree, so the clone can
never contain a dynamic `+` under facts that forbid one. The contract is
enforced at run time twice over: the clone's entry emits a genuine-double tag
check on the accumulator, and the dense entry guard validates the whole masked
window hole-free. A string-seeded accumulator and a string element both route
to the slow copy and produce node's concatenation, verified under forced
evacuation.

Along the way the accumulator walk's index leaf learned masked reads — and
fixed a match-arm reachability bug while doing so: `_ if offset_reads_inlined`
was a guarded catch-all, so any arm placed after it was unreachable whenever
the flag was set. Admitted masked-only single arrays now qualify for
accumulator admission (counter-bearing arrays keep priority; multiple arrays
still decline), and `MaskedWindowArrayFact` carries the admitted accumulators
so `is_numeric_expr` can see them while the clone lowers, mirroring the
string-window fact's field.

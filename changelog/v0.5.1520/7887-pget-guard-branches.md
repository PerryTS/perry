**`perf(codegen)`: the generic property-get guard branches instead of AND-ing eight predicates — `interp` 0.843 → 0.784 s, `iso_miss` 1.231 → 1.178 s.**

The inline monomorphic-IC diamond built one flat `hit` predicate out of eight tests, so LLVM
if-converted the whole region: every receiver executed every load and every compare —
including the two epoch loads — even after the first test had already decided the answer. It
also tested the two *rare* receiver tags (SSO, INT32 class-ref) before the pointer tag,
putting two constant materialisations, two compares and two branches in front of every real
object read.

The chain now branches — `pget.recv_ok` → `pic.recv_hdr` → `pic.token` → `pic.hit` — and the
POINTER/STRING tag test goes first, with SSO/INT32 discriminated in a cold `pget.recv_other`
(the three tag classes are pairwise disjoint, so the order is free). The four header
predicates stay flat on purpose: they are four loads from the same two cache lines and LLVM
fuses their compares into one `ccmp` chain, which beats four branches. `pic.miss` recomputes
what the polymorphic-way compares need from the same memory rather than taking phis, which
would have dragged their `cset`/`csinc` materialisation back onto the hot path.

Semantics are unchanged — every predicate is still checked, with control flow instead of data
flow. `evalNode`'s monomorphic hit path drops from 58 to 46 aarch64 instructions before the
field load.

Measured on the quiet M1 mini, best-of-5, exit-checked, 19-program corpus, both arms linking
the same runtime archives: `interp` −7.0%, `iso_miss` −4.3%, `cycles` −2.8%, `pipeline`
−1.9%, `churn_read` −1.8%, `deeplist`/`retain_wide1` −1.3%. Six binaries compile
byte-identical and set the run's noise floor at ±0.6%.

The guard test walks the def chain rather than asserting presence: it walks the CFG backwards
from the block performing the raw slot load, requires every edge on that path to be the
**true** edge of a `cond_br`, and takes the transitive def closure of those conditions,
requiring each guard to be reachable from a branch condition. Three sabotages were run and all
three went red (constant branch condition, swapped `cond_br` edges, deleted predicate) — a
presence assertion catches none of the first two. The walk must be scoped to a single
function: register names restart at `%r1` in every body, so a global def map resolved the
receiver-tag condition to an unrelated `ptrtoint` in another function.

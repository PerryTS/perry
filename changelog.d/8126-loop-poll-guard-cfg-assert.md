`loop_safepoint_purity`'s guard assertion no longer depends on block print
order. `a_surviving_poll_is_guarded_by_the_arming_word` failed on main while
the lowering was correct: every `load volatile @PERRY_GC_POLL_ARMED` is
followed by `icmp` and `br i1` into a `gcpoll.N` block holding the call.

The test searched the whole remainder of the IR after each load and required
the first `br i1` to precede the first poll call, which assumed each poll block
is printed next to its guard. It is not — the guards sit inline in `for.cond` /
`for.body` / `for.update` while the `gcpoll.N` blocks are emitted together
after the loop — so the leading segments contained no call, `find` returned
`None`, and a correctly-guarded poll was rejected.

The check is now bounded at the load's own basic block: that block must end in
a conditional branch and must not contain the call. The negative controls are
unchanged and still pass, so the assertion keeps its teeth.

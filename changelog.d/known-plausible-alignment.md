**fix(runtime): move the GC-header alignment guard into the delegate, where
every path reaches it.**

`36892b7194` added an alignment check to `try_read_gc_header`, closing a real
UB: the magnitude checks admit in-range garbage such as `0xABCDEF`, and the
deref below is a non-unwinding "misaligned pointer dereference" abort in a
debug build.

But `try_read_gc_header` checks plausibility and then **delegates** to
`try_read_gc_header_known_plausible`, and three sites in
`object/inherited_read_cache.rs` (`:511`, `:666`, `:756`) call that delegate
**directly**. A guard in the wrapper's body covers the wrapper's callers and
misses those three.

Those callers are not at fault — they satisfy the delegate's documented
precondition, which is that `is_plausible_heap_addr(addr)` already holds. The
problem is that the precondition does not carry the guarantee: that predicate
is `is_above_handle_band(addr) && is_valid_obj_ptr(addr)`, and neither term
says anything about alignment. The delegate's safety comment said "As
`try_read_gc_header`, plus …", which stopped being true the moment the
alignment check moved into `try_read_gc_header`'s body rather than into the
shared predicate.

Moved the guard into the delegate, so one check covers both entry points
instead of one check and one gap, and corrected the stale safety contract.
No behaviour change for `try_read_gc_header` — it reaches the same guard one
call deeper. Costs the same single AND on a path that then dereferences.

Two regression tests, both **proven live by sabotage**: removing the guard
fails them (verified, `0 passed; 2 failed`). One pins the delegate — the
direct-call path this fixes — and one the wrapper, so neither entry point can
regress silently. The delegate test is the load-bearing one: without a guard
the failure mode is an abort that takes the whole test binary down, not an
assertion, which is precisely why it has to be refused rather than observed.

Found while auditing the landed `try_read_gc_header_known_plausible` (#10651)
against the newer fix.

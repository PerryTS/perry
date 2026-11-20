### Fixed

**Three more `re-read vs stale local` orderings around the async result promise
(#7497 follow-up to #7516).**

Same family as #7516 — *a value read out of a root and held in a register across
a call that allocates is not rooted* — in the three places that PR left:

* **`js_async_step_done` passed the pre-root copy.** #7516 rooted `trap_next` and
  then handed `resolve_trap_next_with_adoption` the original binding anyway.
  Rooting is not a licence to keep using the old name; that is the exact shape
  the parent PR exists to remove.
* **`resolve_trap_next_with_adoption` held its receiver across
  `js_assimilate_thenable`.** That call runs a *user* `then` getter, and `target`
  is the receiver of both settlements after it — the one of the three with a real
  user-facing path behind it (`return <thenable>` from an async fn, e.g.
  Drizzle's `QueryPromise`, #586).
* **The `AsyncStep` arm seeded `INLINE_TRAP` from its locals.** Between the
  re-read at the top of the arm and the `INLINE_TRAP.set` sit the runaway-reentry
  guard (which allocates a TypeError on its bounded path) and two handle pushes.
  A stale `next` parked in the trap is what `js_async_step_done` later settles and
  returns as the async function's own result promise.

**What this does not do.** #7516 recorded one open residual: a protected run
(`PERRY_GC_PROTECT_FROMSPACE=1`) of the *auto-optimize* binary prints the correct
checksum and then faults inside `js_async_step_done`. All three changes above were
attempts at it and **none of them cleared it**, so the holder is still unfound.
#7516's fragment is amended to say that rather than leaving the impression that
the instrument is clean or that one more re-read would do it. Each change is kept
because it is correct on its own terms, not because it was measured to fix
something.

Re-verified after rebasing onto current `main`: `promise_all_chains` byte-exact
against node 26.5.1 on both link modes, `scripts/auto_opt_app_patterns.sh` 12/12
with no skips, `test_gap_gc_global_builtin_lookup_rooting.ts` plus eight
promise/array/async gap tests byte-exact, `cargo test -p perry-runtime` 1744
passed / 0 failed, `scripts/raw_handle_debt.py` unchanged at 999.

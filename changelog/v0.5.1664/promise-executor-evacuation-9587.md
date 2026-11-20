**`new Promise(executor)` no longer returns a dead promise when the executor
allocates.** Claude Code's first-run setup screen wedged on a fresh `HOME`
100% of the time (#9587); onboarding therefore never wrote `theme` /
`hasCompletedOnboarding` (#9674).

The executor is arbitrary user JS and it runs while
`js_promise_new_with_executor` still owns the promise it is about to return.
cc's dialog helper is the shape that exposed it —
`new Promise((res) => { let z = (y) => void res(y); root.render(ui(z)) })` runs
a whole ink/React render, megabytes of allocation, inside the executor. The
evacuating young collection that lands there MOVES the Promise. The resolving
closures survive correctly (their capture slots are GC slots, so the collector
rewrites them), but `promise`, `resolve_closure` and `reject_closure` lived in
bare Rust locals across `js_closure_call2`, so the function handed the caller
the PRE-COLLECTION address. From-space is reset and handed back to the mutator
at the end of the same cycle, so that pointer names recycled memory, and
`await`ing it goes one of two ways:

* the recycled header decodes as `Fulfilled`, so the `await` never suspends and
  resumes immediately with a garbage value — cc advanced past the onboarding
  dialog nobody had answered; or
* it still decodes as `Pending`, so the async step parks its continuation on the
  dead copy. `resolve()` then settles the LIVE promise, which has no reaction,
  and nothing ever resumes — a silent permanent hang with **no throw and no
  rejection**.

Neither failure raises anything a probe can see: `PERRY_REJECTION_DIAG` is
silent and the whole-heap `PERRY_GC_FROMSPACE_SCAN` reports CLEAN, because at
the moment of the collection the stale address exists only in a Rust register,
and by the time it reaches JS from-space has already been flipped. The
instrument that does catch it is `PERRY_GC_PROTECT_FROMSPACE=1`, which faults on
the stale read with `obj_type=5` (`GC_TYPE_PROMISE`).

All three values are now rooted in a `RuntimeHandleScope` for the duration of
the executor and re-read from their handles afterwards — the discipline
`js_promise_subclass_init` and `new_promise_capability` already follow. Two
smaller holes on the same path close with it: `executor` itself was live across
those allocations (rooted now, and only when it really is a closure, so
`new Promise(42)` still cannot hand the collector a non-pointer), and
`make_resolving_functions` rooted `promise` *after*
`ensure_native_resolving_arity_registered`, which allocates on its first call
per thread.

Regression: `crates/perry/tests/issue_9587_promise_executor_evacuation.rs`
(node-oracle output under a nursery-cap sweep, a
`PERRY_GC_SCHEDULE_RATE=1` forced-collection arm, and a `PERRY_GEN_GC=0`
never-relocates control) plus the parity fixture
`test-files/test_issue_9587_promise_executor_evacuation.ts`.

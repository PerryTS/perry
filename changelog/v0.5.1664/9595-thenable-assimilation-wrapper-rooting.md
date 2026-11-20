### Fixed

- **Thenable assimilation now returns the wrapper promise's post-collection
  address; `util.callbackify` with an object-literal thenable SIGSEGV'd at exit
  (#9539).** ~6,000 `util.callbackify(fn)(cb)` calls whose `fn` returns
  `{ then(resolve) { …allocate…; resolve(v) } }` produced every callback's
  correct result and then died with status 139 in
  `promise::microtasks::pump_protected`, where node exits 0.

  `assimilate_via_then_property` (and the class-vtable arm of
  `js_assimilate_thenable`) allocate the wrapper `Promise`, hand the user's
  `then` two resolving closures capturing it, run that `then`, and NaN-box the
  wrapper out of a bare `*mut Promise` Rust local. The `then` body is user
  code: with the moving nursery a loop safepoint inside it evacuates the young
  generation. The wrapper survives and moves — the closures' raw-`i64` capture
  words are pointer-bearing, so the collector rewrites them and `resolve(v)`
  settles the promise at its NEW address — but the Rust local is not a GC root,
  so the function returned the pre-collection address, i.e. retired from-space.
  `callbackify_outer_thunk` then classified that word as a Promise, rooted it
  and attached its reactions to it, so a dead promise reached the task queue and
  the final microtask checkpoint dereferenced it.

  Every value that outlives an allocation or a user-JS call in these functions
  now lives in a `RuntimeHandleScope` handle — a mutable GC root the evacuating
  minor rewrites in place — and each use re-reads through the handle: the
  returned wrapper, the thenable receiver and its `then` action, the resolving
  closures, `callbackify`'s `returned` across its closure allocations and
  across `js_assimilate_thenable`, and `callable_then_field`'s receiver across
  the `"then"` intern.

  `PERRY_GC_PROTECT_FROMSPACE=1` names the stale object exactly
  (`RETIRED FROM-SPACE … obj_type=5 size=80`, faulting on the
  `obj_type == GC_TYPE_PROMISE` test inside `callbackify_outer_thunk`), and
  `PERRY_GC_MOVING_LOOP_POLLS=0` makes even the unfixed build pass — the control
  that names the moving minor inside `then` as the mechanism. Gap test
  `test_gap_9539_callbackify_thenable_exit_gc.ts` is status 139 on 5/5 unfixed
  runs and exits 0 on 10/10 fixed runs, plus 5/5 each under from-space
  protection, a 1 MiB nursery, forced evacuation and
  `PERRY_GC_MOVING_SAFEPOINT=0`. New unit test
  `gc/tests/runtime_roots/thenable_assimilation.rs` drives a native `then` that
  forces a copying minor before calling `resolve(1)`; unfixed it reports
  `Pending != Fulfilled`. `perry-runtime` is 3011 passed / 0 failed
  single-threaded.

Fixed a stale caller `this` after a moving minor in every runtime guard that
binds the implicit-`this` cell for a callback (#10490).

`js_native_call_method`'s prototype-override early path (#9247) bound the
callee's receiver with a private `ImplicitThisScope` guard that kept the
DISPLACED value — the caller's receiver — in a plain `f64` field and wrote it
back in `Drop`. The callee is user code, so a copying minor inside it relocates
that receiver; a struct field is not a root, and the restore reinstalled a
retired from-space address. The caller's next `this.x` then read `undefined`:
`Object.setPrototypeOf(o, proto); o.run()` where `run` calls an allocating
method threw `Cannot read properties of undefined`, deterministically and with
no GC env knobs, and cheerio 1.2.0 crashed in `_findBySelector` on the 3rd of
200 `load()` iterations. The #9445 sweep rooted every
`let prev = js_implicit_this_set(..)` pair, but a save/restore split across a
guard's constructor and its `Drop` does not have that shape — and the same
shape sat behind the `Array.prototype` callback engines (`DenseThisGuard`,
11 dense methods; `ThisGuard`, 9 `js_arraylike_*` methods), where the caller's
`this` was equally corrupted by an allocating callback.

One shared `object::ImplicitThisScope<'scope>` now replaces all four private
guards. It roots the displaced value in a borrowed `RuntimeHandleScope` and
re-reads that slot in `Drop`, so the restore follows the object through an
evacuation; the borrow forces the scope to outlive the guard. The
prototype-override path also re-reads its receiver after
`clone_closure_rebind_this` (that clone allocates). The accompanying audit of
every implicit-`this` save/restore in the runtime and stdlib fixed six more
displaced values held unrooted across user code: the accessor-receiver override
in the handle-method prototype walk, `new.target` in the Intl and Temporal
subclass `super()` bridges, and ten stdlib sites (domain, events, process
warnings, net, web streams, tls ALPNCallback, worker_threads). The remaining
save/restores — including the 121 rooted by #9445 — were verified rooted.

Validation: `test-files/test_gap_10490_implicit_this_scope_rooting.ts` (17
shapes: `setPrototypeOf`, `Object.create`, `__proto__` literals, class
instances with a swapped prototype, `call`/`apply`, a per-evaluation subclass,
and the dense / array-like callback engines) prints a non-zero `bad=` count on
12 of 17 cases before the fix and is byte-identical to node after it, in both
the default and `PERRY_NO_AUTO_OPTIMIZE=1` pipelines. Four runtime unit tests
in `gc/tests/runtime_roots/implicit_this_scope.rs` plant a callback that runs a
forced-evacuation copying minor and assert the restored cell holds the
receiver's relocated address; the three that exist pre-fix fail on it. The
issue's repro passes under `PERRY_GC_SCHEDULE_SEED=1..5
PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_PROTECT_FROMSPACE=1` (baseline fails all
five), and a scaled copy matches node with `PERRY_GC_SCHEDULE_ALLOC_KB=0` over
120,516 copying minors. cheerio 1.2.0 compiled from source now completes
200 × load + 3 queries with node's exact output. Gap suite: 814/820, the same
6 snapshot entries as the baseline, no new failures. Cost on the changed
dispatch path is one handle slot: +0.71 % instructions on a 5M-call
swapped-receiver microbenchmark, +0.36 % on 5M array-callback engine calls.

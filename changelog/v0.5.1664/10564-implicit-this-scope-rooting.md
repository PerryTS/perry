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

Follow-up review finding (same PR): the rooting fix above makes every displaced
value survive a moving collection, but four sites still save/restore
IMPLICIT_THIS and `new.target` with a bare statement pair, not a guard --
`fetch_globals.rs`'s Temporal/Intl subclass `super()` bridges, the
prototype-walk accessor dispatch in `handle_methods.rs`, and the stdlib
listener/getter dispatchers (`streams.rs` and its `net`/`tls`/`worker_threads`
siblings, `domain.rs`, `events.rs`, `events/warnings.rs`). When the bracketed
call throws, the restore statement textually follows it, so neither `longjmp`
nor a system unwind ever runs it -- both cells stay pinned at whatever the
failed call set them to for every later read.

`exception.rs` already keeps a `catch_savepoints!` family of exactly this
shape (shadow stack, runtime handles, call-method depth, ...): one member per
piece of state a transport-skipped cleanup would otherwise leak, captured at
every `try` and replayed by `js_throw` before it transports the exception --
uniformly for both `js_try_push`/`HandlerKind::Setjmp` and the generated-code
`js_eh_try_push`/`HandlerKind::Unwind`, which already funnel into one
`try_push_with_kind` -> `CatchSavepoint::capture()`, with `js_throw` calling
`.restore()` unconditionally before it branches on transport. `implicit_this`
and `new_target` now join that family. The captured bits are a second root for
whatever heap value they hold while a `try` is open, invisible to the live
cell's own scanner, so `scan_exception_roots_mut` now also walks the live
prefix of the per-thread savepoints slab and rewrites both fields across a
moving collection -- registered in the perex GC test harness too, which
clears the production scanner registry and had only restored the live-cell
scanner.

Proven: a unit test that reproduces the bare save/call/restore shape using
only pre-existing public entry points (`js_implicit_this_set`/
`js_new_target_set`/`catch_js_throw`/`js_throw`, none of them touched by this
fix, so the same test body runs unmodified on both trees) -- an inner
`js_throw` crossing the bare site leaves both cells stuck at the inner value
instead of the enclosing `try`'s baseline. Fails on the pre-fix tree with
`assertion left == right failed: the try open around the bare site must have
restored IMPLICIT_THIS` (left the inner sentinel, right the outer baseline);
passes after, along with the macro's own auto-generated nested-throw witness
for both new members.

NOT proven: that any of the four named sites is reachable the way the finding
assumes. Targeting the most tractable one -- `handle_methods.rs`'s
prototype-walk accessor dispatch -- a temporary `eprintln!` placed directly on
that path never fired for a getter-throws test case built to exercise it, so
something else resolves that call first. The other three sites were not
probed at all. This is recorded, not fixed, in a follow-up issue.

Validation: `cargo test --release -p perry-runtime --tests`
(`RUST_TEST_THREADS=1`): 3975 passed, 1 pre-existing failure
(`a_free_or_move_outside_every_scope_is_caught_in_debug_builds`, a
`debug_assert!` funnel that cannot fire under `--release`, already documented
in this PR's own validation table), 4 ignored. `cargo test --release
-p perry-stdlib --tests`: 138 passed, 1 pre-existing failure unrelated to this
change (`readline::stdin_data_listener_flows_without_raw_mode`; this fix
touches no file in `perry-stdlib`). `scripts/run_lint_gates.sh`
(`SKIP_COMPILE_GATES=1`, compile tier known-red on this host): 76 of 77 pass,
the one red (`Public benchmark evidence freshness`) pre-existing and
unrelated. 10 of 11 targeted exception/try-catch gap tests pass; the one
failure (`test_issue_7302_thread_throws`) reproduces identically on the
unmodified baseline (a Node-side environment artifact in this sandbox, not a
Perry regression). Instruction-count A/B (`perf stat`, 3 runs/arm, spread
<0.1%) on a 5,000,000-iteration try/catch loop that never throws: 895.28M
(baseline) vs 965.34M (fixed) instructions, +7.83% (~14 instructions per
`try`-push) -- the cost of two more TLS reads folded into the one savepoint
write every `try` already performs. Measured on a deliberately adversarial
microbenchmark (nothing but the `try`/`catch` itself); the original PR's own
dispatch-call microbenchmarks, which mix in real work, show proportionally
smaller deltas for comparable per-call additions.

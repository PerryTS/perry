### Layer 1 rooting migration, slice 5 — the timer and namespace-call lowerings (#7615)

`lower_call/extern_timers.rs` and `lower_call/namespace_call.rs` now make every
rooting decision through `crate::rooting`, and are listed in the
`MIGRATED_MODULES` ledger. Both lines are load-bearing on the committed source:
each named `lower_exprs_rooted` / `temp_root_release` before the migration. The
sabotage arm was run per module — a compiling `temp_root_push_double` /
`temp_root_truncate` pair planted in each, the ledger test confirmed red and
naming both lines, with the build output checked for `error[` (**0** in both
arms, so neither plant was a build failure scored as a successful sabotage).

**Four live bugs, found by the audit rather than by the translation.** All four
are demonstrated against node 26.5.1 (the `.node-version` oracle) with a
baseline compiler built from `main` in a separate target directory, and verified
fixed.

1. **Two-argument `setTimeout` / `setInterval` held the callback unrooted across
   the delay expression.** #7210 rooted the *trailing-argument* forms and left
   their two-argument siblings alone, although the comment it wrote names the
   window exactly. The delay is an arbitrary expression, so
   `setTimeout(() => …, churn())` emits

   ```llvm
   %r6 = call i64 @js_closure_alloc(...)      ; the callback
   %r8 = bitcast i64 %r7 to double            ; a BARE register
   %r9 = call double @perry_fn_mod__churn()   ; the delay. User code, polls.
   %r10 = call i64 @js_timer_validate_callback(double %r8, i32 0)   ; stale
   ```

   Compiled with `PERRY_GC_MOVING_LOOP_POLLS=1` and run under `PERRY_GC_ZEAL=1`,
   the baseline throws #7210's own symptom text — `The "callback" argument must
   be of type function. Received an instance of Object` — for both timers, where
   node prints the scheduled timer.

   **Two preconditions, and both are easy to miss.** `PERRY_GC_MOVING_LOOP_POLLS=1`
   is a **compile**-time flag: without it `churn()` gets no loop back-edge polls,
   zeal only fires at event-loop boundaries, a compute-only loop never collects,
   and the identical binary prints the correct answer at exit 0. Check it took
   effect with `--trace llvm`: the module must contain at least one
   `call … @js_gc_loop_safepoint` (1 with the flag, 0 without). And the callback
   must genuinely **capture** — a callback that closes over nothing but module
   globals allocates a closure whose contents nothing relocates, so the stale
   register keeps working. `PERRY_GC_PROTECT_FROMSPACE` and
   `…_DEPTH` are *not* needed here: this fault is a use-after-move that lands on
   recycled memory, not a from-space `SIGSEGV`, so zeal alone surfaces it.

   **The runtime fault is arrangement-dependent; the IR window is not.** Review
   could not reproduce the `TypeError` on a `main` baseline built during the
   audit, using these files and these commands — it printed the correct answer.
   What review *did* confirm, directly in `--trace llvm` output, is the window
   itself: `%r9 = bitcast i64 %r8 to double` (the callback), then
   `call double @perry_fn_…__churn()`, then
   `js_timer_validate_callback(double %r9, …)` reading the register defined above
   the call — and the fixed arm storing `%r8` into a root slot and reloading it.
   Whether a stale pointer is *observably* wrong depends on what gets recycled
   into those bytes, which is why the acceptance tests assert the IR ordering
   rather than a runtime outcome. **A window that does not fault today is not a
   window that is absent** — it is one whose victim happened to survive.

   Conversely, a window that does not fault is not a window that is absent —
   whether a stale pointer is *observably* wrong depends on what gets recycled
   into those bytes. The IR above is the evidence that the bug is there; the
   thrown `TypeError` is only evidence that it is reachable in one arrangement.

2. **The var-shaped namespace export dispatched through a stale closure.**
   `import * as ns from "./m"; ns.arrow(churn())` fetches the closure from its
   zero-arg getter first (spec order — the callee reference is evaluated before
   the arguments), holds it in a bare register across every argument's lowering,
   and only then `unbox_to_i64`s it into a raw heap address. This is #7280
   taxonomy (a) and (c) at once: `root_reload` could not have repaired it, because
   the pointer is derived *below* the window from a register captured *above* it.
   Baseline throws `TypeError: value is not a function`; node and the fix print
   `a:1` / `b:2`. Same two preconditions as bug 1 (compile-time polls; zeal alone
   suffices).

   The reproducer that faults imports a **`.ts`** sibling and calls a two-argument
   export. A `.mjs` sibling with a one-argument export emits the identical window
   — getter, `churn`, then `bitcast`/`and` of the pre-call register into
   `js_closure_call1` — and does **not** fault, for the recycling reason above.
   Worth knowing before concluding from one non-faulting arrangement that the arm
   is clean.

3. **The `has_rest` namespace direct call lost every rest element — silently, on
   the default build, with no GC instrumentation at all.** This is the most
   serious of the four and the only one that needs nothing special to see.
   The #7154 accumulator shape verbatim: `current` was a raw `*mut ArrayHeader`
   threaded through a push loop while the next argument's expression ran, holding
   the only reference to everything pushed so far. `lower_rest_call_args_rooted`
   was written for exactly this and this path never adopted it. For
   `lib.joinRest(churn("head"), churn("r1"), churn("r2"), churn("r3"))` node
   prints `head|r1,r2,r3` and the baseline prints `head|`. Independently
   reproduced during review with a different repro shape, where the baseline
   prints **nothing at all and exits 0** — no zeal, no protect, no
   `PERRY_GC_MOVING_LOOP_POLLS`, just the plain compiler: the argument churn
   allocates enough to guarantee a collection inside the window, so unlike the
   other three this one needs no instrumentation to arrange. A wrong answer, not
   a crash. Delegating to the audited helper also pads the fixed parameters to
   the declared arity, which the hand-rolled loop did not.

4. **Three more unprotected windows**, repaired in passing and reachable by
   inspection rather than by a reproducer that faults today: the `fs/promises`
   `writeFile` / `appendFile` / `rmdir` arms (operand-to-operand — `path` held
   across `content` and `options`), both V8-bridge arms (a bare
   `for a in args { lower_expr }`, #7240's shape in a path that post-dates the
   fix), and the plain namespace direct-call argument loop.

**Cost is zero where the window cannot collect**, and that is now pinned rather
than asserted. A literal delay routes the callback to `OperandProtection::Reuse`,
and the emitted module for `setTimeout(fn, 5)` / `setInterval(fn, 5)` contains no
temp-root traffic at all — the callback register feeds
`js_timer_validate_callback` directly, exactly as before.

**Tests.** `lower_call/timer_rooting_tests.rs` asserts on emitted IR rather than
on runtime behaviour, because the runtime fault needs a capturing callback, a
polling delay *and* the compile-time `PERRY_GC_MOVING_LOOP_POLLS=1` (off by
default since #7161) — a gap test would be green on the default build whether or
not the fix is present, which is hazard 4. The assertion is an *ordering*, not a
slot count: with an allocating delay the register `js_timer_validate_callback`
reads must be defined below the delay's allocation, and with a literal delay it
must be the original register with zero temp-root traffic. Checked against the
pre-fix source: the two ordering tests fail, the two zero-cost tests pass, so
neither is vacuous.

**Four modules of the family were deliberately not migrated**, and the reason is
recorded in the ledger comment because it is a statement about the API: three of
them need a re-read at more than one point and every `with_operands_rooted*` form
has exactly one. `mod.rs` returns a guard on purpose (its consumers in
`func_ref.rs` are block-splitting specialized-ABI diamonds whose release must sit
in a merge block ~200 lines below); `new.rs` re-reads one operand group at three
caller-chosen points under a scope marker spanning ~20 return paths;
`console_promise.rs`'s `lower_dynamic_closure_call` re-reads in two stages; and
`early_branches.rs`'s only escape-hatch uses are the already-paired
`implicit_this_save`/`restore`, so migrating it would be a rename that made the
ledger line look substantive while asserting nothing. The concrete missing
combinator is the variadic/rest shape: per-element re-reads between allocating
pushes.

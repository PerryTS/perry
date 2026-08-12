### Layer 1 rooting migration, slice 6 — the multi-point re-read scope (#7615)

`crate::rooting::RootedGroup` is the combinator slice 5 could not build:
**one temp-root scope — already-lowered operands and mutable accumulator arrays
together — re-readable at any number of caller-chosen points and released once,
for the whole stack.** `lower_call/mod.rs`, `lower_call/func_ref.rs` and
`lower_call/console_promise.rs` are migrated onto it and listed in the
`MIGRATED_MODULES` ledger; all three named `expr::temp_root` before the
migration, so all three lines are load-bearing on the committed source.

**Slice 5's hypothesis for the missing shape was wrong in a way worth
recording**, because the next slice would have inherited it. It named the
variadic/rest shape (per-element re-reads between allocating pushes). Only one
of the three blocked modules is variadic:

* `console_promise.rs`'s `lower_dynamic_closure_call` consumes the group in
  **two instructions with an allocating step between them** — receiver and callee
  feed `js_closure_unbox_callee_checked_rebind`, which clones a `this`-capturing
  closure, and the arguments feed `js_closure_callN` below it;
* `mod.rs`'s `lower_rest_call_args_rooted` re-reads in a **loop**, one per
  `js_array_push_f64`;
* `func_ref.rs`'s **release** must post-dominate four block-splitting
  specialized-ABI dispatch diamonds, ~450 lines below the lowering.

What all three want is the scope, not the loop. The variadic case is the scope
that also holds an array, which is why `RootedGroup` carries both kinds rather
than there being a second type for it.

**Two entry points, and the asymmetry is argued in the source rather than
assumed.** `with_rooted_group` owns the release like every other combinator in
`rooting.rs`. `open_rooted_group` hands the scope back, which the rest of the
file deliberately refuses to do, and the justification is that the two halves of
guard mismanagement are not equally dangerous. An **early or mis-ordered**
release is a use-after-free — a truncate is a stack *cut*, so truncating the
wrong slot drops every slot above it, which is how a saved implicit `this`
becomes the number `0`. A **forgotten** release is over-retention: the slot stays
bound, the object stays live, the emitted code is merely conservative.
`RootedGroup` removes the dangerous half by construction and for both entry
points — it is not `Clone`, `release` consumes it, and there is no way to obtain
the slot index — so escaping leaves only the safe half writable. That is
strictly better than the `Option<String>` slot index it replaces, which a caller
could truncate anywhere. Inverting control in `func_ref.rs` would not remove the
hazard, only relocate it into a 450-line closure.

`implicit_this_save` / `implicit_this_restore` **moved** into `crate::rooting`
rather than being re-exported, so the pair has one spelling; two spellings of one
decision is the drift that produced #7114. That incidentally clears the escape
hatch out of `early_branches.rs`, `method_override.rs` and both `property_get`
dispatchers, which are deliberately **not** added to the ledger: a line there
asserts that a module makes every rooting decision through this API, and those
four have not been read for windows with no decision at all (slice 4's
"listed ≠ audited" distinction).

**Four live bugs in the `console.*` arms.** The three behavioural ones are
A/B'd byte-for-byte against node 26.5.1 with a baseline compiler built from
`main` in a separate target directory; `diff` of node's output against the fixed
arm is empty, and against the baseline it is four hunks.

1. **`console.dir` sequenced side effects after its own print** — #7649's non-GC
   arm, which the issue flagged as unverified. It lowered `args[2..]` *below*
   `js_console_dir_with_options`, so `console.dir(x, y, sideEffect())` printed
   the object and only then ran `sideEffect`. Node evaluates a call's whole
   argument list before invoking anything.
2. **`console.time` / `timeEnd` / `timeLog` / `count` / `countReset` dropped
   their surplus arguments entirely.** Not resequenced — never lowered.
   `console.time("t", sideEffect())` simply did not run `sideEffect`.
3. **`console.table(a, b, c)` stopped being `console.table`.** The arity gate was
   `args.len() == 1 || args.len() == 2`, so three or more arguments fell through
   to the generic multi-argument `console.log` arm and printed
   `[ { a: 1 } ] [ 'a' ] 1` where node renders the table. Node ignores the
   surplus arguments; it does not switch renderer.
4. **#7649's rooting half**, demonstrated in IR. `console.table(data, properties)`
   and `console.dir(obj, options)` held operand 0 in a bare SSA register across
   operand 1's lowering. For `console.table(makeRows(), [churn(300)])` the
   baseline emits `%r1 = call @makeRows()`, `%r2 = call @churn(300)` (user code:
   allocates, polls), then reads `%r1` — and `root_reload` structurally cannot
   repair it, because a call result has no slot to be re-read from (#7280
   taxonomy (c) and (d) at once).

   **The runtime fault is arrangement-dependent and was not reproduced.** Under
   `PERRY_GC_MOVING_LOOP_POLLS=1` at compile time plus `PERRY_GC_ZEAL=1`
   (`PERRY_GC_DIAG=1` confirms `copied_objects=6005`, so the subject was live),
   and again with `PERRY_GC_PROTECT_FROMSPACE=1 …_DEPTH=800`, the baseline
   printed the correct table and exited 0. The IR is the evidence that the window
   exists; a thrown `TypeError` would only have been evidence that it is
   reachable in one arrangement.

**Two more unprotected windows in the same module, found by reading the arms the
migration did not have to touch** — which is the "listed ≠ audited" distinction
being paid for rather than restated:

* **`Promise.try(cb, ...extra)` was #7154's accumulator shape verbatim.**
  `current_arr` was a raw `*mut ArrayHeader` threaded through the push loop in a
  bare SSA register, holding the only reference to everything pushed so far while
  the NEXT argument — arbitrary user code — was lowered, and `callback` sat in
  another bare register across `js_array_alloc`, every push and every one of
  those lowerings. Identical to the `namespace_call.rs` rest-path defect slice 5
  called the most serious of its four. The pre-fix IR threads
  `%r55 → %r60 → %r65` through three `churn()` calls with no root; the fixed IR
  re-reads the accumulator from its slot between every push.
* **`Array.fromAsync(input, mapFn, thisArg)` held three operands across each
  other's lowering** — #7280 taxonomy (c), which `root_reload` cannot repair.
  One re-read point serves, so this is the plain `with_operands_rooted` form.

Neither faults in the arrangements tried (including `PERRY_GC_ZEAL=1` on a
`PERRY_GC_MOVING_LOOP_POLLS=1` build); the IR ordering is the evidence.
`Array.fromAsync` deliberately keeps `take(3)`, so this is a pure rooting fix:
the `Promise.*` statics and `Array.fromAsync` all fail to EVALUATE their surplus
arguments, which is the same node-visible defect as `console.time`'s above, and
fixing that family belongs in its own change with its own oracle A/B.

**Cost is zero where the window cannot collect, and it is measured.** A probe
exercising direct calls, rest calls, `arguments`, `new`, method dispatch,
dynamic closure calls, `Map.set` and eight `console.*` arms was compiled under
both compilers with `--trace llvm`. Normalising SSA numbering and block labels,
the entire semantic delta is **35 removed lines and 0 added**: two unconditional
`temp_root_push_double` / re-read / clear sequences protecting values
`expr_is_known_non_pointer_shadow_value` proves are not heap references — a
`double` from a scalar-replaced method call, and the literal `true` in
`console.assert(true, …)`. Both arms bypassed the shared `operand_protection`
decision; routing them through it drops the traffic. Over the
`gc_root_dominance_corpus.sh` corpus the root-store count falls 9846 → 9799 with
zero change in violations.

**Tests.** `lower_call/console_rooting_tests.rs` asserts on IR *ordering* rather
than slot counts — a count would let the other operand's rooting pay for the
assertion — and each test first asserts by callee name that the arm under test
was reached, so a shape measured over a lowering that never ran cannot pass.
Sabotage-verified against the pre-fix source with the file reverted to its `HEAD`
content: `error[` count 0, `Running unittests` present (so the plant compiled
*and* the test binary was reached), 4 of 5 red. The fifth is the zero-cost pin
and correctly passes in both arms. The ledger sabotage arm was run once per newly
listed module — a compiling `temp_root_push_double` / `temp_root_truncate` pair
planted in each, `error[` count 0 in all three, ledger test red and naming both
planted lines.

**One gate observation, recorded rather than fixed.**
`scripts/gc_root_dominance_check.py --seeded-violations N` runs only after the
real check returns 0, and without `--moving-only` the corpus reports 171
violations (all non-moving, the
`js_object_alloc_class_inline_keys → js_gc_declare_typed_shape_layout` class) on
`main` and on this branch alike — so a run without that filter never reaches the
"can this gate still fail?" arm and prints nothing about it. In the gate's own
mode (`--moving-only`) it is exercised and reports 40 planted, 40 caught, 0
missed, with 0 violations in both arms.

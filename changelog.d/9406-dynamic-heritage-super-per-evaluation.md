### Fixed

- **A factory that returns `class D extends <its parameter>` no longer
  SIGSEGVs when it is chained through its own previous result.** The five-line
  repro is zod v4's `$constructor` shape, the single most-used class factory in
  the `claude-code` bundle:

  ```js
  function mk(P) { class D extends (P ?? Object) {} return D; }
  const A = mk(null);
  const B = mk(A);
  console.log("ok " + typeof new B());   // node: ok object -- perry: SIGSEGV
  ```

  One level was fine; the second level died, and only when the derived class
  was actually instantiated. Not a regression — it reproduced identically on
  `83754818e` (#9242) and `a03be729c` (#9336).

  The recursion is a `super()` chain that never descends.
  `CLASS_DYNAMIC_PARENT_VALUE` — the stash a compiled constructor's `super()`
  leg reads back through `js_get_dynamic_parent_value` — is keyed by the
  **template** class id and is last-wins, so one class evaluated N times leaves
  exactly one heritage recorded. When that heritage is an *earlier evaluation
  of the same template*, the parent's constructor re-reads the same entry,
  resolves the same parent, and re-enters itself until the stack guard page.
  Two lowerings reach it, and each needed its own half of the fix.

  **A non-capturing function-body class DECLARATION** (no captures, no private
  elements, no computed keys) keeps the shared-template lowering: it has no
  per-evaluation class object at all, so `mk(null) === mk(A)` and the second
  evaluation stashed `ClassRef(D)` against D itself.
  `js_register_class_parent_dynamic` already rejects `parent_cid == class_id`
  for the registry edge it writes ("so a recursive helper that returns its
  receiver can't create a cycle"); the VALUE stash beside it did not. It does
  now — a class is never its own superclass, and rejecting the write keeps
  whichever heritage the earlier evaluation recorded, the only heritage a
  single class id can describe.

  **A capture-carrying declaration or a class EXPRESSION** does materialize a
  distinct class object per evaluation, and each already pins its own heritage
  (`js_class_object_pin_parent`) — the per-evaluation prototype chain and the
  per-evaluation capture snapshot both read it from there. The `super()` leg
  could not: a compiled constructor knows only its template class id, so it
  asked the template stash and got the LAST evaluation's parent at every level.
  `new B(x)` replayed B's constructor, resolved A, replayed A's constructor,
  resolved A again, and looped. The constructor replay now names the evaluation
  it belongs to for the duration of the call, and `js_get_dynamic_parent_value`
  answers from that evaluation's pinned heritage when one is active.

  Affected files:

  - `crates/perry-runtime/src/object/class_registry/evaluation_heritage.rs`
    (new) — the self-heritage predicate and the active-replay frame, with the
    NaN-boxed class objects the frames hold.
  - `crates/perry-runtime/src/object/class_registry/parent_static.rs` — reject a
    self-heritage stash write; split `js_get_dynamic_parent_value` into the
    per-evaluation override plus `template_dynamic_parent_value`, which
    `js_class_object_pin_parent` keeps using so a pin still records what the
    class DEFINITION evaluated.
  - `crates/perry-runtime/src/object/class_constructors.rs` — push the frame
    around the class-object constructor replay, keyed on the same
    `capture_owner` object that supplies the constructor's capture params. The
    guard pops on unwind.
  - `crates/perry-runtime/src/object/class_registry/gc_roots.rs` — the frames
    hold live heap pointers across a user constructor body, so the class
    side-table root scanner visits and forwards them.

  Validation: `test-files/test_gap_9364_factory_decl_dynamic_parent_chain.ts`
  plus byte-comparison against node 26.5.1 over 20 probes — both lowerings, one
  / two / three chain levels, an explicit `super()`, a rest-parameter
  constructor, a declared-class parent instead of `Object`, static state on the
  derived class, and the full zod `$constructor` shape (`Object.defineProperty`
  on `name`, an initializer closure, `instanceof`). All previously-SIGSEGVing
  probes now match node. `perry-runtime` 2895 passed / 0 failed. Five focused
  unit tests cover the stash guard (including a ClassRef to a *different* class,
  which must still be recorded) and the override (including that it answers only
  for the replaying class's own template id, and that the frame pops); each half
  was sabotage-checked — disabling the guard fails exactly two, disabling the
  override fails exactly one. A 20,000-iteration construction loop over a
  three-level chain runs clean under `PERRY_GC_SCHEDULE_SEED=999
  PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_PROTECT_FROMSPACE=1` (20,005 copying minors,
  20,000 loop polls, from-space `mprotect`ed), which is what exercises the new
  root.

  `scripts/run_lint_gates.sh` passes all 60 gates (including
  `gc_runtime_root_holders.py`, which is what the new root has to satisfy). The
  full gap suite reports 597/611 with 14 output mismatches; five are the
  committed snapshot entries and the other nine reproduce on the pristine merge
  base under the identical procedure — `test_gap_6336_class_expr_builtin_parent`
  is reproduced there by adding two `eprintln!`s to `perry-runtime` and nothing
  else, i.e. that host's ext-wrapper archives go incoherent on any runtime edit.

  Two adjacent gaps are deliberately NOT addressed here and remain open. A
  shared-template class declaration still collapses its evaluations, so
  `mk(null) === mk(A)` reads `true` where node says `false` (and therefore
  `Object.getPrototypeOf(B) === A` reads `false`); giving that shape a
  per-evaluation class object is a lowering change with a far wider blast
  radius than a crash fix should carry. Separately, a single evaluation of
  `class D extends (P ?? Object) { constructor(d) { super(d); this.d = d; } }`
  loses `this.d` — that reproduces unchanged on the merge base and is not
  introduced or worsened here; the chained form used to SIGSEGV and now reaches
  the same pre-existing wrong value.

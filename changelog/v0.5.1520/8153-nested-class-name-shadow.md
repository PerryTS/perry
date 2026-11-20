### Fixed

- **`new C()` inside `C`'s own method constructed an unrelated local when an
  enclosing scope had a same-named binding.** A `class C` declared inside a
  nested function, referenced by `new C()` from one of its own method bodies,
  while some enclosing scope also declares `var C` / `let C`, threw
  `TypeError: undefined is not a constructor` at runtime. Node runs it fine —
  the class's own name binding is the nearest one.

  Two arms of the lowering disagreed about the same identifier. The bare-ident
  read arm (`arm_ident.rs`) already applied the JS nearest-binding rule via
  `forward_class_names` + `forward_class_decl_depth`, so a plain `C` inside the
  method correctly resolved to `ClassRef("C")` — `typeof C` returned
  `"function"`. The `new <Ident>` arm did not: it snapshotted
  `ctx.lookup_local("C")` unconditionally, found the *enclosing* scope's
  binding, and rerouted the construct to
  `NewDynamic { callee: LocalGet(<outer slot>) }`. A method compiles to its own
  function, so that slot index names an unrelated, uninitialized local there —
  the callee evaluated to `undefined` and the construct threw.

  The failure is silent up to that point: the class registers, its methods
  exist, and every reference to the name *other than* `new` resolves correctly.

  Affected files:

  - `crates/perry-hir/src/lower/context.rs` — new
    `LoweringContext::forward_class_shadows_local`, the nearest-binding rule as
    one predicate: a local in the CURRENT scope always wins; otherwise the
    binding at the greater scope depth wins.
  - `crates/perry-hir/src/lower/lower_expr/arm_ident.rs` — the read arm now
    calls that predicate instead of carrying its own copy, so the two arms
    cannot drift again.
  - `crates/perry-hir/src/lower/expr_new.rs` — suppress the local-callee
    snapshot (and the later re-lookup that could resurrect it) when the class
    binding is the nearer one.

  The depth rule is what keeps the case the reroute exists for: a module-scope
  `class e` does **not** beat a factory-local `let e`, so mysql2's bundled
  chunk still constructs the local's value.

  Found while triaging #8040 (a production Next.js App Route serving empty
  bodies with `TypeError: active is not a function`). Next 16 ships this shape
  in the webpack chunk that inlines `@opentelemetry/api`: the module IIFE
  declares `var g,h,i,j,…` and later assigns each of them a module-exports
  object, while an inner factory declares
  `class i { static getInstance(){ return this._instance || (this._instance = new i), this._instance } active(){ … } }`.
  `new i` constructed the outer `i` — a plain exports object — so
  `js_new_function_construct` fell back to a `class_id = 0` empty object.
  `context` was therefore a non-null object with no prototype at all, which is
  why the tracer's `(context == null ? void 0 : context.active())` guard let it
  through and the request died on `active`.

  The same file shows why the symptom looked like a prototype bug and moved
  under unrelated edits: the collision rename accidentally immunised every
  DUPLICATE single-letter class (`i$0`, `i$1`, … match no local), so only the
  first `class <letter>` of each name was affected. In that bundle
  `ContextAPI` (`class i`) and `PropagationAPI` (`class l`) were broken while
  `TraceAPI` (`class j`, renamed) worked.

  Validation: `cargo test -p perry-hir` — 314 lib tests plus every integration
  suite, exit 0. Sabotage: with the guard forced off, both regression tests
  fail with the exact defect (`NewDynamic { callee: LocalGet(…) }` in the
  static method's body); the two over-trigger guards pass either way, which is
  their job.

  End-to-end: a harness built from the fixture's own
  `.next/server/chunks/2.js` reproduces `TypeError: active is not a function`
  and, with only the compiler swapped and the runtime archives held fixed,
  flips to node's output — `context`/`propagation`/`trace`/`diag`/`metrics` all
  carry their prototypes, `getTracePropagationData()`, `isOpenTelemetryEnabled()`
  and `trace()` all return node's values.

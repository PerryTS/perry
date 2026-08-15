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
  bodies). Next 16 ships this shape in the webpack chunk that inlines
  `@opentelemetry/api`: the module IIFE declares `var g,h,i,j,…` and an inner
  factory declares
  `class i { static getInstance(){ return this._instance || (this._instance = new i), this._instance } active(){ … } }`.
  `getInstance()` threw, the module factory aborted mid-initialization, and the
  webpack module cache then handed the tracer a `{}` for
  `@opentelemetry/api` — so `context` never got its `active()`.

  Validation: `cargo test -p perry-hir` — 312 lib tests + every integration
  suite green. Sabotage: with the new guard forced off, the regression test
  fails with the exact defect, `NewDynamic { callee: LocalGet(0) }` in the
  static method's body. The companion test (`factory_local_still_shadows_…`)
  passes either way by design — it exists to catch over-triggering, not to
  detect this bug.

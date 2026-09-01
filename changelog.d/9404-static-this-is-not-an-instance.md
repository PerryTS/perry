### Fixed

- **Inside a `static` body, `this` is no longer treated as an instance of the
  class.** `class P { m() { return 1; } static probe() { return typeof this.m; } }`
  answered `"function"`; node answers `"undefined"`. Worse than the `typeof`:
  `this.m()` in a static body *succeeded*, running the instance method body with
  the class ref as its receiver, where node throws a `TypeError`.

  Two independent defects produced that one symptom, and each is reachable on
  its own.

  **1. The codegen type predicates typed static `this` as an instance.**
  `receiver_class_name(Expr::This)` and `static_type_of(Expr::This)`
  (`crates/perry-codegen/src/type_analysis/predicates.rs`) both answered
  `Named(class_stack.last())` in a static body exactly as they do in an instance
  body. `class_stack` names the owning class in a static body too — that is what
  `super.x` resolves against — but a static body's `this` is the class
  CONSTRUCTOR: an INT32 class ref, never a heap instance. Every consumer of
  those two answers was therefore entitled to prove instance facts about the
  constructor object: instance field slots, shape ids, direct method dispatch.

  `Named(C)` is not merely imprecise here, and "the constructor object of C"
  would not have been a better answer: static members are INHERITED, so `this`
  in a static body of `Base` is whatever subclass the call came through
  (`Sub.inherited()` sees `this === Sub`, and `Sub` may override every static
  member the body touches). `None` is the only sound answer, and it is what both
  predicates now return under `FnCtx::in_static_member`.

  This is what closes the alias residual #9386 documented and left open:
  `static viaLocal() { const t = this; … }` reached the computed-member route
  through `guarded_declared_class_get_candidate`, which reads `local_types` —
  written by `refine_type_from_init` from `static_type_of`. With that predicate
  honest the wrong type never enters `local_types`
  (`G.viaLocal()`: `undefined|` → `object|9`).

  **2. The runtime's constructor-side property walk read `C.prototype`.**
  Declared instance methods are mirrored onto the reflective `C.prototype`
  object as own data fields. `resolve_proto_chain_field` walks that object, and
  the CONSTRUCTOR-side read in `js_object_get_field_by_name` (`C.foo` on a class
  ref, after own statics and the static-method chain miss) called it — so every
  prototype method resolved on the class object. This needs no `this` at all:
  on `dcf1ec0fbc`, `class P { m(){} }` gave `typeof P.m === "function"` and
  `P.m === P.prototype.m`, via the dot, computed, and `Reflect.get` forms alike.
  `js_object_has_property` already had the gate (`"m" in P` was correctly
  `false`), and the `is_prototype_ref` gate in the same file plugged this hole on
  the direct-vtable door for #1021/NestJS — this is that door's chain-walk twin.

  The receiver-less `resolve_proto_chain_field` has exactly one caller and it is
  that static-side read, so the exclusion is applied there rather than at the
  call site. It is keyed on `class_instance_has_member` — the exact "is this a
  prototype method / getter / setter of the chain" predicate — and NOT on "skip
  the decl-prototype entirely". A blanket skip was tried first and is wrong: it
  also removes `C.constructor`, which the decl-prototype carries as an ordinary
  data field. That answer is load-bearing today for a reason outside this issue:
  **perry hands a PROPERTY DECORATOR the class itself where the spec hands it
  `Class.prototype`**, so NestJS-style
  `Reflect.defineMetadata(k, v, target.constructor)` relies on
  `C.constructor === C`. Node says `C.constructor === Function`, so perry has two
  divergences that cancel, and removing either alone breaks decorator metadata —
  measured: `test_decorators_nest_common_canary` and
  `test_decorators_legacy_property_metadata` both went pass -> parity_fail on the
  blanket version. The decorator-target defect is the one worth fixing, and it is
  not this issue.

  The `class_prototype_object` step of the same walk is never skipped: for a
  subclass of a class-EXPRESSION value it holds the parent CLASS OBJECT
  (#1788/#6552), which is genuinely on the constructor's static chain.

  Fixing only (1) would have left the issue's own example broken, and would have
  moved one shape — `const t = this; typeof t.computedMethod` — from
  accidentally-right to wrong, because it stopped taking the computed-member
  route (which answered `undefined` for the wrong reason) and joined every other
  instance-member read on the leaking generic path.

  Affected files:

  - `crates/perry-codegen/src/type_analysis/predicates.rs` — a guarded
    `Expr::This if ctx.in_static_member => None` arm ahead of each existing
    `Expr::This` arm.
  - `crates/perry-codegen/src/type_analysis_facts.rs` —
    `CodegenTypeFacts::this_type` carries the same gate. Without it the generic
    HIR inference (`infer_expr_type`) re-derived `Named(C)` for every expression
    that merely *contains* `this`, routing around `static_type_of`'s refusal.
  - `crates/perry-runtime/src/object/class_registry/prototype_objects.rs` —
    `resolve_proto_chain_field_inner` takes `skip_decl_prototype`, set for the
    constructor-side form only.

  No fast path is lost for the operations a static body actually performs.
  Static field reads through `this` (`this.sf`), static method calls through
  `this` (`this.other()`), `this.prototype` and `this.name` were already on the
  generic class-ref dispatch: `class_field_global_index` never matched a static
  field, and `resolve_static_dispatch_cls` has no `Expr::This` arm —
  deliberately, because static inheritance means `this` in a static body cannot
  be resolved to the declaring class at compile time.

  Validation: `test-files/test_static_this_is_not_an_instance_9404.ts`,
  byte-compared against `node --experimental-strip-types`, covering a static
  method, a static block, a static getter, `this === C`, static-to-static
  dispatch through `this`, the same on a subclass where `this` is the *sub*class,
  the `const t = this` alias (plain and computed member), an instance-side
  control, and a static method whose name collides with a String method.

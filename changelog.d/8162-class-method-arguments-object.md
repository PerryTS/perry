### Fixed

- **`arguments` in a class method: the remaining call-site shapes after #8082 (#8040).**
  A class method whose body reads `arguments` received an array holding
  `max(0, argc - declaredParams)` entries instead of all of them — the
  `arguments` slot #677 synthesizes is a trailing `is_rest` parameter, exactly
  how a user `...rest` is spelled, so the compile-time-resolved class-method
  call sites bundled it from `declared - 1`, the offset a *user* rest wants.
  #8082 landed the synth-vs-rest split for three of the four affected sites
  (the guarded direct call and the per-implementor arm in
  `lower_call/property_get/dynamic_dispatch.rs`, and `StaticMethodCall` in
  `expr/static_method.rs`). This change carries the remainder:

  * **`super.m(…)`** (`expr/super_method.rs`) did no bundling at all — every
    argument went positionally, so the parent's trailing array slot received a
    raw scalar. That also mis-served a plain `super.m(1, 2, 3)` into
    `m(a, ...rest)`.
  * **A method with BOTH a user `...rest` and an `arguments` read** declares
    `[a, rest, arguments]` — TWO trailing array slots bundled from different
    offsets over the same argument list. `(has_rest, has_synthetic_arguments)`
    cannot express that (`has_rest` is true for either spelling), so the
    synth arm won and the rest slot received a scalar:
    `m(a, ...rest) { arguments }` called as `m(1, 2, 3)` bound `rest` to the
    number `2`. A new `method_has_user_rest` bit (`codegen/arguments.rs`, read
    off the defining class's HIR — `arguments_object` is present on the
    synthesized parameter and on nothing else) sizes the tail at every direct
    call site: dynamic dispatch (all three arms via `build_direct_method_args`),
    `StaticMethodCall`, and `super.m(…)`.
  * **`js_array_mark_arguments_object`** is now emitted over the synthesized
    bundle at these sites, matching the freestanding-function path
    (`lower_call/func_ref.rs`) and the #5703 static-dispatch slice — without it
    the callee's `arguments` is an ordinary Array and fails every
    arguments-object predicate.

  Found bringing up a production Next.js App Route: Next.js bundles
  OpenTelemetry's `NoopTracer.startActiveSpan`, whose first statement is
  `if (arguments.length < 2) return;` — under the conflation that guard fired
  on every well-formed three-argument call, so `tracer.trace()` returned
  `undefined` without invoking its callback and the route answered with an
  empty body.

  Regression coverage: `crates/perry-codegen/src/expr/class_method_arguments_object_tests.rs`
  (IR census on the call site — asserts the synthesized array is filled from
  argument 0 *and* marked, plus the negative that a user `...rest` still
  bundles only its trailing args and is not marked) and
  `test-files/test_gap_arguments_in_class_method.ts` (byte-for-byte against
  Node — instance, static, inherited, async, generator, `super.m(…)`,
  dynamic/`call`/`apply` control arm, the `startActiveSpan` guard shape, and
  the rest+`arguments` both-case by value, not just by length).

### Fixed

- **`arguments` inside a class method dropped every argument but the trailing ones (#8040).**
  A class method whose body reads `arguments` received an array holding
  `max(0, argc - declaredParams)` entries instead of all of them:
  `m(a, b) { return arguments.length }` called as `m(1, 2, 3)` answered `1`, and
  `arguments[0]` was the *third* argument. Only a method declaring zero
  parameters — the shape every existing `arguments` test happened to use — was
  accidentally correct, which is why this survived so long.

  Root cause: `arguments`-synthesis (#677) appends a hidden trailing parameter to
  such a method and marks it `is_rest`, which is exactly how a user `...rest` is
  spelled. The class-method call sites keyed off that single bit, so they bundled
  it from `declared - 1` — the offset a *user* rest wants — while the synthesized
  slot must be filled from argument 0 and marked with
  `js_array_mark_arguments_object`. The freestanding-function path
  (`lower_call/func_ref.rs`) has always emitted the correct shape, and
  `lower_call/property_get/static_dispatch.rs` was fixed for its own slice in
  #5703. Four sites were not: the guarded direct call and the per-implementor
  subclass arm (both `lower_call/property_get/dynamic_dispatch.rs` — the second
  is the one a call made from inside another class method reaches), the
  `StaticMethodCall` path (`expr/static_method.rs`), and `super.m(…)`
  (`expr/super_method.rs`), which passed every argument positionally so the
  callee's trailing array slot received a raw scalar. Runtime dynamic dispatch (`o[name](…)`,
  `.call`, `.apply`) was already correct, because the runtime method table
  carries a separate `has_synth_args` flag — so the bug reproduced only through
  compile-time-resolved calls.

  All four now resolve the trailing-parameter shape from the callee's own HIR
  (`arguments_object` is present on the synthesized parameter and on nothing
  else) and emit accordingly, including the case where a method has both a real
  `...rest` and an `arguments` read — two bundles over the same argument list at
  different offsets, which previously left the user rest bound to a scalar.

  Found while bringing up a production Next.js App Route. Next.js bundles
  OpenTelemetry's `NoopTracer.startActiveSpan`, whose first statement is
  `if (arguments.length < 2) return;`. Under the conflation that guard fired on
  every well-formed three-argument call, so `tracer.trace()` returned `undefined`
  without ever invoking its callback: the route's generated handler resolved
  having never entered `routeModule.handle`, and the request was answered with an
  empty body.

  Regression coverage: `crates/perry-codegen/src/expr/class_method_arguments_object_tests.rs`
  (IR census on the call site — asserts the array is filled from argument 0 *and*
  marked, plus the negative that a user `...rest` still bundles only its trailing
  args and is not marked) and
  `test-files/test_gap_arguments_in_class_method.ts` (byte-for-byte against Node,
  the class-method twin of the existing object-literal test, which had asserted
  this same property since #321).

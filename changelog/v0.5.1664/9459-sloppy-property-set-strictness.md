### Fixed

- **A rejected `o.x += 1` / `for (o.x of …)` / `[o.x] = arr` no longer throws in
  sloppy code.**

  ```js
  // sloppy (.cts, no "use strict")
  const o = {x:1}; Object.freeze(o);
  o.x += 1;            // node: silent   Perry: TypeError
  for (o.x of [7]) {}  // node: silent   Perry: TypeError
  [o.x] = [7];         // node: silent   Perry: TypeError   (expression position)
  o.x++;               // node: silent   Perry: silent  (correct — Expr::PropertyUpdate)
  o.x = 9;             // node: silent   Perry: silent  (correct — Expr::PutValueSet)
  ```

  ES2024 §6.2.5.7 (`PutValue`) performs `Set(O, P, V, Throw)` with
  `Throw = IsStrictReference(ref)`, and §10.1.9 (`OrdinarySet`) reports `false`
  — not a throw — for a non-writable own or inherited data property, an
  accessor with no setter, and a new property on a non-extensible object. The
  reference's own strictness is what turns that `false` into a `TypeError`.

  This is the ordinary-object mirror of #9394 (arrays, fixed by #9426) and the
  opposite direction from #9422 (an *under*-throw in strict code). A CommonJS
  bundle is sloppy from top to bottom, so this was a hard failure — a program
  node runs to completion stopped with a spurious `TypeError`.

  Root cause: `Expr::PropertySet` carries **no strictness field at all**, and
  its codegen tail reaches `js_typed_feedback_object_set_field_by_name_fast` →
  `js_object_set_field_by_name`, which has no `strict` parameter and rejects by
  throwing unconditionally. `o.x++` was already right because it lowers to
  `Expr::PropertyUpdate`, which carries `ctx.current_strict`; `o.x = 9` was
  already right because it lowers to `Expr::PutValueSet`, which carries
  `strict`. Only the spellings that lower to `Expr::PropertySet` — compound and
  logical assignment, `for`-of heads, and expression-position destructuring
  targets — had no answer to give.

  The flag comes from the **context**, exactly as #9426 did for
  `Expr::IndexSet`: `ctx.is_strict_fn` at the ordinary dispatch, and
  `PutValueSet::strict` at the two sites that synthesize a `PropertySet` from a
  `PutValue`. That is deliberately not a new HIR field — `Expr::PropertySet` has
  181 mentions across the workspace (119 constructions, 54 of them in production
  code), and a large minority of those live in collectors and transform passes
  that *rebuild* an existing node with no strictness context to copy from. A
  field would have needed a default at each of those, which is precisely where a
  wrong answer hides. `FnCtx::is_strict_fn` is already the audited answer for
  the enclosing code: `Function::is_strict`, `Expr::Closure::is_strict`,
  `Module::init_is_strict` (#9458), and a hard `true` for class methods.

  Sloppy stores route to `js_put_value_set(target, key, value, receiver, 0)` —
  the receiver-aware `[[Set]]` that sloppy `o.x = v` has always used — so the
  three spellings now agree instead of diverging by lane. The class-field fast
  arm is preserved through `try_lower_sloppy_class_field_store` (#7288/#5094),
  whose #5093 inline precheck declines every receiver whose store could be
  rejected, so the fast path is mode-independent and only its miss needed a
  sloppy-correct tail. Strict lowering is byte-identical to before.

  - `crates/perry-codegen/src/expr/dispatch.rs` — `Expr::PropertySet` passes
    `ctx.is_strict_fn`, the twin of the `Expr::IndexSet` line above it.
  - `crates/perry-codegen/src/expr/proxy_reflect.rs` — the two `PutValueSet`
    routes into `property_set::lower` pass the reference's own `strict`.
  - `crates/perry-codegen/src/expr/property_set.rs` — `lower` takes
    `assignment_strict`; the `arr.length` arm (`js_array_set_length_strict`) and
    the class-field arms (`js_class_field_set_ic` /
    `js_class_field_set_fallback`) are strict-only; a new
    `lower_sloppy_property_set_by_name` emits the `js_put_value_set(…, 0)` tail
    with the same #7154 receiver-rooting window and the same nullish-receiver
    guard the strict tail uses (`undefined.x = 1` is a `TypeError` in both
    modes — `GetValue` on the base runs before `PutValue` consults `Throw`).
  - `test-files/test_gap_9459_property_set_strictness.cts` — `+=`, `-=`, `*=`,
    `&&=`, `||=`, `??=`, `for`-of heads (named and computed), `[o.x] = arr` in
    statement *and* expression position, `[o[k]] = arr`, `({a: o.x} = obj)`,
    against frozen / sealed / non-writable own / inherited non-writable /
    getter-only own and inherited / inherited-setter / `preventExtensions` /
    frozen class-field / frozen `arr.length` receivers — **both modes**, with
    accepted-store and short-circuit controls so a fix that simply stopped
    storing would fail.

  Two things this deliberately does not change, both pre-existing on `main` and
  both visible in the fixture's own comments:

  - `caller` / `arguments` keep their `js_object_set_field_by_name` route in
    both modes. `PutValueSet` sends those two names here specifically to reach
    that entry's poisoned-accessor handling, which is not a `Throw`-flag
    decision.
  - `+=` against a receiver whose rejection lives on the **prototype** (an
    inherited non-writable data property, an inherited getter-only accessor, an
    inherited setter) is still wrong in **strict** code: the strict tail does an
    own-property store and never runs `OrdinarySetWithOwnDescriptor`'s
    prototype walk, so the setter does not fire and an own property is created.
    That is a missing prototype walk rather than a missing `Throw` flag, it is
    wrong on unfixed `main` in both modes, and fixing it means retargeting the
    typed-feedback store site that #7480/#5093 gate with their own IR tests.
    Filed as #9495. The sloppy half becomes correct here as a side effect of
    routing to `js_put_value_set`, and the fixture pins the strict `=` twins so
    that change has a baseline.

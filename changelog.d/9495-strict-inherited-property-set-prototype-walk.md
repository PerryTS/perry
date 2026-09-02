### Fixed

- **Strict `o.x += 1` against an INHERITED non-writable property, getter-only
  accessor or setter now runs the prototype walk.**

  ```js
  "use strict";
  const proto = {};
  Object.defineProperty(proto, "x", { value: 10, writable: false, configurable: true });
  const a = Object.create(proto);
  a.x += 1;        // node: TypeError        Perry: silent, created own a.x = 11

  const g = {};
  Object.defineProperty(g, "x", { get() { return 20; }, configurable: true });
  const b = Object.create(g);
  b.x += 1;        // node: TypeError        Perry: silent, created own b.x = 21

  const calls = [];
  const s = {};
  Object.defineProperty(s, "x", { get() { return 30; }, set(v) { calls.push(v); }, configurable: true });
  const d = Object.create(s);
  d.x += 1;        // node: setter runs with 31, no own property
                   // Perry: setter NEVER ran, created own d.x = 31
  ```

  ES2024 §10.1.9.2 (`OrdinarySetWithOwnDescriptor`): when the receiver has no
  own property, `[[Set]]` walks to the parent and the *parent's* descriptor
  decides — a non-writable data property rejects, a getter-only accessor
  rejects, a setter runs with the original receiver, and only a writable data
  property (or the end of the chain) creates a new own property. `PutValue`
  then throws on a rejection iff the reference is strict.

  `o.x = v` was already right — it lowers to `Expr::PutValueSet` →
  `js_put_value_set`, whose `ordinary_set_with_receiver` walks the chain — and
  #9459 made the *sloppy* half of these spellings right as a side effect of
  routing the sloppy `Expr::PropertySet` tail to that same entry. Only the
  **strict** spellings that lower to `Expr::PropertySet` — compound and logical
  assignment, `for`-of heads, expression-position destructuring targets — and
  the strict object-by-name arms of `Expr::IndexSet` (`o["x"] += 1`,
  `o[k] += 1`) were still wrong. This is a missing prototype walk, not a
  missing `Throw` flag: the opposite direction from #9422 and a different
  defect from #9459.

  Root cause: the strict tails ended in
  `js_typed_feedback_object_set_field_by_name_fast` /
  `js_typed_feedback_object_set_field_by_name` → `js_object_set_field_by_name`,
  an **own-property** store. The shape-transition fast path inside it already
  declines any receiver whose prototype is not the ordinary one, so every one of
  these receivers fell to the slow branch — which appended an own property
  without ever consulting the chain.

  Fix: the strict tails now reach the same receiver-aware `[[Set]]` the sloppy
  tails and the `=` lane use, `js_put_value_set(target, key, value, receiver,
  strict)`, so the two modes are one tail distinguished by the `Throw` flag
  alone. The typed-feedback `PropertySet` site moves with the store: it is
  registered in both modes (it describes the store, not its strictness) and
  observed by the pure-recording `js_typed_feedback_observe_property_set`,
  compile-gated on `PERRY_TYPED_FEEDBACK` exactly as #7480 step 4 gates every
  other recording helper — a default build emits the bare `js_put_value_set`
  call and nothing else. Nothing was left for the old dispatching wrapper to
  decide (the receiver-aware entry makes the fast-path choice itself), so the
  #7480 "dispatching wrappers still emitted in a default build" gate is
  re-pointed at the method-call dispatcher the same fixture emits, and asserted
  as a *call* rather than a symbol (the symbol match was satisfied by the
  `declare` line alone).

  - `crates/perry-codegen/src/expr/property_set.rs` —
    `lower_put_value_property_set_by_name` replaces the sloppy-only helper
    and is the generic tail for both modes; `caller` / `arguments` keep their
    `js_object_set_field_by_name` route (poisoned-accessor handling, unrelated
    to either flag or walk). `emit_typed_feedback_property_set_observation`
    carries the site.
  - `crates/perry-codegen/src/expr/index_set.rs` —
    `lower_object_index_set_put_value` replaces the sloppy-only helper on the
    literal-string-key and string-typed-key object arms.
  - `crates/perry-codegen/src/expr/typed_feedback.rs` —
    `TypedFeedbackContract::put_value_set`.
  - `test-files/test_gap_9495_strict_inherited_property_set.cts` — the three
    inherited receivers plus a two-level chain, a class accessor on the chain
    and a Proxy on the chain (receiver forwarded), across `+=`, `&&=`, `??=`
    (short-circuit control), `for`-of heads, destructuring in statement and
    expression position, `o["x"]`, `o[k]`, `o[anyKey]`, `[o[k]] = arr`, with
    accepted-store controls (inherited writable data, new key beside an
    inherited accessor, class-ref receiver) and the already-correct `=` /
    `++` lanes — **both modes**, so "silent because sloppy" and "silent because
    the walk never ran" are told apart by the setter-call log and `hasOwn`.
  - `test-files/test_gap_9459_property_set_strictness.cts` — the strict
    inherited twins are spelled `+=` now, as that file's comment promised.

  Left as it was: `js_class_field_set_fallback` (the class-field arm's
  guard-miss path) and `js_object_set_field_by_property_id` (the
  computed-runtime-members class route) are still own-property stores; neither
  is reachable for the receivers above without a class-typed variable holding
  an `Object.create`d value, and each is its own lane. A DECLARED `static`
  field losing `K.n += 1` in both modes (found while building the fixture) is
  a static-slot lane defect, not a walk, and is filed as #9526.

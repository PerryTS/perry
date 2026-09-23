### Fixed

- **A class expression with dynamic heritage evaluated inside a function was
  one shared class across every evaluation** (#11042). `function mk(Base) {
  const C = class extends Base {}; return C; }` called with two different
  bases returned the SAME class: one class id, one parent edge (the last
  evaluation's `extends` won), one `Class.prototype[name] = …` table. Only
  class expressions that also carried per-evaluation statics, captures,
  private elements or a used self-binding took the per-evaluation
  `ClassExprFresh` path; `#10622` had widened that to an exported factory whose
  whole body is `return class extends X {}`, and same-module call sites were
  cloned by `specialize_captured_class_factories`. Everything else — a
  comma-declarator binding, a factory reached through a function value, a
  cross-module factory with more than one statement — kept the shared template.
  `@redis/client`'s `commander.js` `attachConfig` is exactly that shape
  (`const RESP = …, Class = class extends BaseClass {}` followed by a
  `Class.prototype[name] = …` loop), evaluated once for `RedisClient` and again
  for its Multi command class before the client is constructed, so `new
  Client(options)` ran the Multi class's constructor and the client lost its
  EventEmitter ancestry: `client.on("error", cb)` resolved to the `events`
  module's static `on(emitter, name)` and threw `The "emitter" argument must be
  an instance of EventEmitter. Received type string ('error')`.

  `crates/perry-hir/src/lower/lower_expr/arm_class.rs` now routes every
  function-body class expression with a dynamic `extends_expr` through
  `ClassExprFresh`, whose per-evaluation heritage pin (#6438/#9364/#10624) and
  per-evaluation `.prototype` already existed. A statically resolved parent
  (`extends_expr == None`) cannot differ between evaluations and keeps the
  shared template. The new `perry_hir::class_value_template_name` recognizes
  such a *bare* `ClassExprFresh` (no statics, keys, captures or self-binding)
  wherever a pass read a factory's returned `ClassRef`
  (`factory_specialize.rs`, `lower/misc.rs`), so same-module per-call-site
  specialization and static `extends Factory(...)` parent hoisting are
  unchanged.

  Routing more classes through the fresh path exposed a pre-existing leak:
  `console.log` / `util.inspect` printed the runtime-internal
  `__perry_ctor_class_object` / `__perry_parent_class` pin keys that every
  other reflective surface already hides. The object formatter
  (`perry-runtime/src/builtins/formatting.rs`) now skips
  `is_internal_runtime_key_bytes` keys too.

  Known remaining (pre-existing, not changed here): instances still carry
  their template's class id, so a class-id-keyed lookup can see a sibling
  evaluation's parent — e.g. `mk(A)` instance reading a method that only
  `mk(B)`'s base defines resolves it instead of `undefined`, and
  `Object.create(new (mk(R))()) instanceof R` is `false` after another
  evaluation. Neither blocks redis.

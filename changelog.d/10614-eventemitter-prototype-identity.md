### Fixed

- **`class Sub extends EventEmitter {}` now links `Sub.prototype`'s `[[Prototype]]` to the real
  `EventEmitter.prototype` object (#10599).** `Object.getPrototypeOf(Sub.prototype) === EventEmitter.prototype`
  read `false`: `class_decl_prototype_value` resolves a registered parent class id by recursing into itself,
  which bails for a RESERVED native-builtin parent id (`EventEmitter` has no `js_register_class_name`
  registration of its own), so the link silently fell through to `Object.prototype`. `instanceof` and the
  class-chain walk (#10592) already worked — this is specifically the `[[Prototype]]` object identity, which is
  a separate mechanism.

  The fix resolves the real, closure-identity-keyed prototype through `js_function_prototype_value_for_read` —
  the same helper the existing runtime-function-valued-parent branch already used — so the identity comparison
  holds, not merely the shape. It only takes effect together with a codegen-side parent-edge registration
  (`builtin_parent_reserved_class_id`): #10592 added that entry for plain `EventEmitter`; this PR adds the
  missing `EventEmitterAsyncResource` counterpart, without which `get_parent_class_id` never resolves and the
  runtime fix is unreachable.

  Verified with the runtime fix reverted (twice, independently) that every `getPrototypeOf`/`instanceof`/`in`
  assertion in the new gap test reads `false` where Node reads `true`, and with it restored the test is
  byte-identical to Node 26.5.1. Two pre-existing, unrelated gaps surfaced while writing the test and were left
  out of it: `EventEmitterAsyncResource.prototype`'s own chain to `EventEmitter.prototype` (a
  native-to-native link, not a user subclass), and `Object.keys(new Sub())` leaking `EventEmitter.prototype`'s
  methods as own enumerable instance properties instead of Node's real `_events`/`_eventsCount`/`_maxListeners`
  fields (CLAUDE.md's documented "native base's surface is installed at `super()` time" weak area) — both
  reproduce identically with the fix reverted, so neither is caused by it.

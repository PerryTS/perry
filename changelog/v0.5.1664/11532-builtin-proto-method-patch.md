Honor replaced builtin prototype methods on arrays, Map/Set and functions
(#11394). `Array.prototype.push = f`, `Map.prototype.get = f` and
`Function.prototype.bind = f` were ignored by every call of the method: a
proven receiver folded to an intrinsic (`ArrayPush`, `MapGet`) and a dynamic
one was matched by name in `js_native_call_method`, neither reading the
prototype slot.

The #10848 whole-program pre-scan now also records writes onto
`Array`/`Map`/`Set`/`Function.prototype` (direct, via `globalThis`, through a
`const AP = Array.prototype` alias, `Object.defineProperty`/`Object.assign`,
or a dynamic key; index keys are not method patches). A call of a patched
method name lowers as a property-get-then-call in HIR, and codegen routes it
(spread form included) to the new `js_native_call_method_patched_proto`, which
performs `Get(recv, name)` on array/Map/Set/function receivers and calls a
user function it finds with the receiver as `this`, falling back to the
unchanged by-name dispatch otherwise. The patched-name set is part of the
object-cache key. Programs that patch no builtin prototype lower exactly as
before.

Regression fixture: `test-files/test_gap_11394_builtin_proto_method_patch.ts`.

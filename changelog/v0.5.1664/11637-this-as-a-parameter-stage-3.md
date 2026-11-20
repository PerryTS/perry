The implicit-`this` cell is gone (stage 3 of passing `this` as a parameter):
a body's `this` parameter is the only way it learns its receiver, so no
caller saves, sets or restores anything around a call.

- Deleted: the per-agent `IMPLICIT_THIS` cell, `js_implicit_this_get`,
  `_get_sloppy` and `_set`, `ImplicitThisScope`, its exception savepoint,
  its `HotTls` field and agent-pointer slot, the codegen save/restore
  funnel (`rooting::implicit_this_*`), the stage-1 witness
  (`PERRY_THIS_WITNESS`), and `js_closure_call1_receiverless` (identical to
  `js_closure_call1` once no cell exists).
- A plain call passes `undefined`: `js_closure_call0..16`,
  `js_native_call_value` and `js_closure_call_array`. A call with a
  receiver uses the new `js_closure_call_this0..16`,
  `js_native_call_value_this` and `js_closure_call_array_this`. perry-ffi
  gains `JsClosure::call_this0..4` and `JsThis::{UNDEFINED, as_f64,
  from_f64}` (additive).
- Runtime routes that used to carry a receiver through the cell now pass it:
  `util.promisify`/`callbackify`/`deprecate` wrappers call the original with
  their own receiver (Node's `ReflectApply(original, this, args)`), the
  legacy `Intl.NumberFormat.call(obj)` / `DateTimeFormat` chain, event
  listeners (the emitter), stream and socket methods, N-API
  `napi_call_function`. One route never had it and is fixed: an accessor
  defined on an `arguments` object now runs with that object as `this`
  (`test-files/test_gap_this_param_receiver_routes.ts`).
- Async and generator function expressions bind their receiver at entry like
  any other body. They run once per call; the step closures that run across
  resumptions capture it lexically.
- `this` in module top-level code is `undefined` in strict code and
  globalThis in sloppy code, the value the cell held when nothing set it.
- A method's entry-resolved callback target
  (`js_closure_resolve_plain_direct_call`, formerly `_arrow_`) now admits
  ordinary functions too: the call passes the plain-call `undefined`
  receiver itself, which is all `js_closure_callN` did for them.

Instructions per call (LTO-off build, same host), main -> this change: a
closure value call 145 -> 46, a two-argument closure call 160 -> 59, a
method-site hit on an object literal 99 -> 79, an object-literal method
reading `this` 223 -> 120, an ES5 prototype method 214 -> 111.

`setTimeout`, `setInterval` and `setImmediate` now return real objects instead of
small registry integers. The value used to be an id NaN-boxed with `POINTER_TAG`
— a number pretending to be a pointer — and that id was not even unambiguous:
timer ids and `perry-ffi` registry handles share the same band and both count
from 1, so a live server handle `1` and a `setTimeout` id `1` were the same
value and the method dispatch had to guess between them. That guess is gone.

A timer handle is now a `GC_TYPE_OBJECT` with a family class id, a real ShapeId
and a per-family prototype, matching node's shape measured on 26.8.1:
`Timeout.prototype` owns exactly `close, constructor, hasRef, ref, refresh,
unref` plus `Symbol.dispose` and `Symbol.toPrimitive`, while
`Immediate.prototype` owns `constructor, hasRef, ref, unref` plus
`Symbol.dispose` and NO numeric conversion (`+setImmediate(...)` stays `NaN`,
#10542). `t.constructor.name` is `Timeout` / `Immediate` as before, but it is a
real prototype property now rather than a fresh `{ name }` object fabricated on
every read. `clearTimeout` / `clearInterval` / `clearImmediate` take the handle
or its numeric id exactly as before (#1213).

Node's timer methods are LENIENT on a foreign receiver — measured, not assumed:
`Timeout.prototype.unref.call({})` returns the receiver rather than throwing,
and `hasRef.call({})` is `undefined`. The prototype methods answer the same.

Seven dispatch arms keyed on `is_known_timer_id` are deleted with the
representation: three property-read funnels, the fused-call dispatch, the
`Symbol.dispose` symbol-read arm, and the `+timeout` numeric-coercion arm. The
ref-state registry also stops recording each id's `Timeout`/`Immediate` kind,
because the handle now carries it.

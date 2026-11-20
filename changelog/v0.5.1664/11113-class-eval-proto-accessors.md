fix(runtime): a capture-carrying class's prototype now reflects its ClassBody accessors (part of #11043)

A class whose members close over a local of the enclosing function lowers to
`ClassExprFresh`, and each evaluation materializes its own prototype object
(`class_evaluation_prototype_value`). Like a declared class's prototype, that
object carries physical `constructor` + method keys while `get`/`set`
accessors live only in the template vtable — but it was never recognized by
`class_id_for_decl_prototype_object`, the lookup every reflection site uses to
surface those accessors. So `Object.getOwnPropertyNames(C.prototype)` omitted
them, and `Object.defineProperties(C.prototype, { x: { enumerable: true } })`
(whatwg-url's generated `URL` wrapper) installed a read-only `undefined` data
property over `get x`/`set x`. mongodb 7.5.0 compiled from real source then
threw `Cannot assign to read only property 'pathname'` from
`mongodb-connection-string-url`'s `ConnectionString` constructor.

`class_id_for_decl_prototype_object` now falls back to
`class_evaluation_prototype_class_id`, which recognizes a per-evaluation
prototype structurally (own `constructor` is a heap class object of the same
template id whose hidden evaluation-prototype slot points back at it) — no
side table to root or rekey, no allocation, and gated on an `AtomicBool` so
programs without such classes pay one relaxed load on the miss path.

Gap test: `test_gap_11043_class_eval_proto_accessors`. The next mongodb blocker
is #11111 (`net.Socket#write` returns `undefined`); #11112 tracks the `in`
operator on the same class shape.

**`Object.prototype.toLocaleString` now gives an accessor `toString` the primitive receiver.**
The spec routes it through `Invoke(O, "toString")` → `GetV(O, "toString")` →
`ToObject(O).[[Get]]("toString", O)`, and that third argument is the *receiver*:
the original primitive, not the wrapper object the lookup walked through. Perry
resolved the method with a plain get **on the prototype**, which is
unobservable for a data property but runs an **accessor**'s getter with the
prototype as `this` — so
`Object.defineProperty(Boolean.prototype, "toString", { get() { … } })` saw
`typeof this === "object"` where the spec requires `"boolean"`
(test262 `built-ins/Object/prototype/toLocaleString/primitive_this_value_getter.js`).
An accessor is now resolved explicitly and its getter invoked through
`call_primitive_closure_value`, the helper the callers already use for the
resolved method: it hands a raw primitive `this` to a strict callee and a boxed
wrapper to a sloppy one. Both resolvers on the path — the resolve-and-call
`call_primitive_builtin_prototype_method` and the resolve-only
`builtin_proto_user_method` — share the new helper so they cannot drift.
Covered by `test-files/test_gap_object_tolocalestring_primitive_receiver.ts`.
Still divergent, and independent of this rule: a `toString` resolving to a
**non-callable** should throw, while Perry falls through to the native method —
that reproduces identically through a plain data property, because the resolver
collapses "absent" and "present but not callable" into one answer. (#5901)

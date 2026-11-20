A receiver with a per-instance `[[Prototype]]` override no longer loses the
methods Perry synthesizes rather than storing on a real prototype. The
override fast path assumed the resolved method was a user closure, so a
property-lookup miss called `undefined` (`[...gen().map(f)]` threw
"undefined is not iterable"), a field-get miss hid the plain-function
`.prototype` and the boxed-wrapper builtins, and a native method was invoked
with no receiver bound (`Object.prototype.isPrototypeOf` and
`Object(true).valueOf()` both threw).

The fast path now runs only for a resolved callable, a field-get miss defers
to the rest of the lookup instead of answering `undefined`, and the receiver
is bound for the duration of the call. A resolved hit still wins over the
class vtable, so an explicit `Object.setPrototypeOf` is unaffected.

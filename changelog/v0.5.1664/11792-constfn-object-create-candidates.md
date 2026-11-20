### Object.create values call the prototype's method body directly

A value made by `Object.create(P)`, where `P` is a `const` bound to an object
literal, now has `P`'s literal class as its compile-time candidate. The
candidate follows the value through `const` bindings (including captured and
module-level ones) and through calls of a function whose every `return` is
such a value. Its method sites dispatch a learned inherited ConstFn hit whose
recorded body is `P`'s method body directly, so the call can be inlined. They
emit no compare against `P`'s own shape, because the receiver inherits the
method. The candidate is never trusted: the receiver word and the prototype's
word still decide the hit, and a replaced method, another prototype or an own
property takes the generic path.

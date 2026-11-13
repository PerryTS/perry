`Object.assign(target, ...)` with a primitive target returns a wrapper object
again in the compiled output. The whole-program facts modelled the result as
the target itself, so `Object.assign(5, { x: 1 })` was proven an int32 5:
`typeof` said "number", `.x` read null and `valueOf()` gave -2147483648 (node:
"object", 1, 5). The result is now ToObject(target) in both lowerings (the
`Object.assign(...)` expression and a value call of the builtin): an object
target's own objects, and a wrapper object for a primitive one; `undefined` /
`null` add nothing (they throw). The runtime already returned the wrapper.

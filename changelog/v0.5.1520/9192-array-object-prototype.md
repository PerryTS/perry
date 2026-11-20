### fix(runtime): an array retargeted to a non-array prototype inherits from it

`Object.setPrototypeOf(arr, someObject)` recorded the new `[[Prototype]]` —
and paid the process-wide array-index deoptimisation for it — but the lookup
paths then declined to consult it: the index probe accepted a recorded
prototype only when the prototype was itself an array, and the named-property
fallback hardcoded `Array.prototype`. A retargeted array therefore inherited
nothing from its new prototype while still inheriting everything from the old
one, with no error: `a[7]` and `a.foo` were `undefined`, `7 in a` was `false`,
and `typeof a.map` was still `"function"`.

Index reads/`in`/writes, named reads, `in` on a name, `arr.constructor`,
`arr.__proto__`, symbol-keyed reads, and method dispatch (`arr.first()`, the
ES5 `MyList.prototype = Object.create(Array.prototype)` idiom) now all resolve
through the recorded chain, with the array bound as the receiver so a
prototype accessor observes the right `this`. `Object.setPrototypeOf(arr,
null)` correctly inherits nothing at all. Named properties on an *array*
prototype, which never resolved either, are fixed by the same change. Arrays
with the default prototype are untouched. Fixes #9192.

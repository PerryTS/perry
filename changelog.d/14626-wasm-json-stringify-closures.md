`--target web`: `JSON.stringify` returned `undefined` for every value, a closure
written inline under most expressions (a `.map` callback followed by `.join`, a
replacer, a static field initializer) reached the runtime as `undefined`, and
serialized class instances carried an internal `__class__` key. `JSON.stringify`
now honors its value, replacer and space; every closure is emitted once; and
`__class__` is non-enumerable.

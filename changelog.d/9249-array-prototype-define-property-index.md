### fix(runtime): honor prototype index descriptors installed with defineProperty

Indexed array assignments now observe accessors and non-writable data
properties installed on `Array.prototype` or `Object.prototype` through
`Object.defineProperty`, `Object.defineProperties`, or
`Reflect.defineProperty`. Descriptor installation raises the same prototype
invalidation latch as a plain indexed write, and the strict store fallback now
walks the default prototype chain before creating an own element. Fixes #9249.

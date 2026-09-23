Key-adding and shadowing stores on class instances (`this.x = …` in a
constructor with no field declarations, `this.m = this.m.bind(this)`) no
longer run the full `[[Set]]` each time: the store site keeps the verdict
that the prototype chain does not intercept the key, keyed by the prototype
identity the receiver's shape records, and appends through the shape
transition. [[Prototype]] is now a shape fact: `setPrototypeOf`,
`__proto__`, `new F()` and `Object.create(p)` all give the object a shape
naming its prototype. Also fixes a stale store plan that skipped an
inherited setter after `F.prototype` was replaced.

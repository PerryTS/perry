**[[Prototype]] is part of shape identity.** A shape is canonical per
(prototype, ordered keys): `setPrototypeOf`, `__proto__`, `new F()` after
`F.prototype` is replaced, `Object.create(p)` and class evaluation each move
the object to the shape naming its new prototype, and two objects with the
same keys but different prototypes never share a shape. The shape generation
now tracks only descriptor, delete and freeze changes.

Key-adding and shadowing stores on class instances (`this.x = …` in a
constructor with no field declarations, `this.m = this.m.bind(this)`) no
longer run the full `[[Set]]` each time: the store site keeps the verdict
that the prototype chain does not intercept the key, keyed by the prototype
identity the receiver's shape records, and appends through the shape
transition. Also fixes a stale store plan that skipped an inherited setter
after `F.prototype` was replaced.

**Behaviour change: a method read off `this` is the class's method, not a
receiver snapshot.** `const f = this.m` now answers the same canonical
function as `obj.m` and `C.prototype.m` (`this.m === C.prototype.m` holds),
and the value binds no receiver — calling it bare runs with `this`
undefined, as in Node. This retires the #4548 snapshot contract, under
which every `this.m` read built and named a fresh bound closure and a
captured `this.m` kept its receiver after an own-property replacement. The
constructor self-rebind `this.m = this.m.bind(this)` that #4548 fixed keeps
working.

zod: −32.7% instructions and −30% peak RSS; tsc transpile: −3.1% instructions (one run).

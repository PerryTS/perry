`super.m()`, `typeof super.m`, `super.m` as a value and `super.x` now read the
parent prototype as it is when they run, so a patched, deleted or getter-backed
parent method applies, and so does `Object.setPrototypeOf` on the class
prototype or, in a static method, on the class itself. A static `super.s()` no
longer calls an instance method of the same name. After a class prototype is
relinked, `instanceof` follows the new chain, including `instanceof Object` and
an instance whose own prototype was replaced. `C.prototype.__proto__ = X` now
relinks the prototype like `Object.setPrototypeOf` instead of creating an own
property named `__proto__`.

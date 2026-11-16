On the web target `super.m()` in an object-literal method calls the method on
the literal's current prototype instead of returning `undefined`, and throws a
`TypeError` when that prototype is null or `m` is not callable. A literal written
with `__proto__: p` (or with a method that uses `super`) keeps its own keys and
its prototype.

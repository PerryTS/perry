A dynamic method call or method read on an instance of `class D extends Date`
(`d[name]()`, an untyped `x.getTime()`, `const f = d.getTime`) resolves on the
instance's own prototype chain, `D.prototype` before `Date.prototype`. Both
paths read the realm's `Date.prototype` directly, so a subclass override was
skipped and `d.constructor` was `Date`.

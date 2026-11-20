`instanceof` on a function object whose prototype was reset
(`Object.setPrototypeOf(fn, C.prototype)`) now follows that chain: `fn
instanceof C` is true, and `fn instanceof Function` is false unless the new
chain reaches `Function.prototype`. Function objects keep a reset prototype
in the closure side table, which these checks ignored.

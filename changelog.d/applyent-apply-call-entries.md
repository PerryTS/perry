`f.apply(…)` and `f.call(…)` on a function receiver whose `Function.prototype`
method is unmodified run the intrinsic directly: a compiled body is entered
with its arguments (an `arguments` object, a packed Array or `null`/`undefined`
list read without a heap copy), instead of the by-name method dispatch.
`f.apply(this, arguments)` whose lookup answered "by name" takes the same
entry after re-checking the receiver. Async, generator and async-generator
functions are admitted too: their kind's intrinsic prototype is sealed once its
parent is wired, so an unshadowed word proves it still defines none of the
methods. schema-record −6.8%, effect −2.7% instructions.

The borrowed-call rewrites (`Math.max.apply(…)`, `Array.prototype.join.call(…)`,
`(function(){…}).call(…)`, `Promise.resolve.call(…)`) no longer run when the
program writes that `Function.prototype` method; the call reads the property.

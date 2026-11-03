`Function.prototype.call.call(f, ...)`, `Function.prototype.apply.call(f, ...)`
and `Function.prototype.bind.call(f, ...)` (also through a local holding the
intrinsic) now run `f` itself. They used to be compiled as the by-name
`f.call(...)`, so a function relinked by `Object.setPrototypeOf` onto an object
with its own `call`/`apply`/`bind`, or one with an own `call`, ran that method
instead of its body.

Regression tests for `Function.prototype.call.call(f, ...)`,
`Function.prototype.apply.call(f, ...)` and `Function.prototype.bind.call(f, ...)`
(also through a local holding the intrinsic): they run `f` itself, even when `f`
was relinked by `Object.setPrototypeOf` onto an object with its own
`call`/`apply`/`bind` or has an own `call`. Tests only; no behaviour change.

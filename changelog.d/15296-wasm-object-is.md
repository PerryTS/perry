On `--target web`, `Object.is(a, b)` returns SameValue (`NaN` is `NaN`, `0` is not `-0`) in synchronous code and in an `async` body, instead of `undefined` for every input (#15103).

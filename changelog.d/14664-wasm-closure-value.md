On the web target a closure used as a value is a function: `typeof` gives
`"function"`, and a closure stored on an object runs when called through it
(`o.h()`, `({ f() { ... } }).f()`) instead of returning `undefined`.

`--target web`: `==` was always `false` and `!=` always `true`, whatever the
operands, because the emitter called a `js_loose_eq` bridge the runtime never
defined. It is defined now, so `x == null`, `"1" == 1` and the `?.` guard
(`base == null`) follow JS. A compiled class method read as a value (`o.m`,
the `o.m?.()` guard) is now a function — the same one each time — instead of
`undefined`; the broken `==` had been letting `o.m?.()` through by accident.

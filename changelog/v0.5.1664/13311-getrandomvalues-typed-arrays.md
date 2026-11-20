`crypto.getRandomValues()` now fills every integer TypedArray in place and
returns the same object (#12819). It used to work only for `Uint8Array` and
`Buffer`: an `Int32Array` stayed zeroed, a different object came back, and
destructuring the result threw `TypeError: value is not a function`. The call
now uses the same runtime path as `crypto.randomFillSync()`, both through the
global `crypto` and through the `node:crypto` namespace.

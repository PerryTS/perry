### fix(runtime): honor inherited array indices in writes and borrowed methods

Indexed assignment on an array with a recorded custom prototype now performs
the inherited descriptor walk before creating an own element. Prototype
setters therefore run with the array as their receiver, inherited non-writable
data properties reject the assignment, and inherited writable data properties
still allow the normal own-property creation.

The generic array-like engine used by `Array.prototype.<method>.call(array)`
now uses the same recorded-prototype classification for `Get` and
`HasProperty`. Prototype-filled holes are consequently visible to `join`,
`indexOf`, `map`, `forEach`, and the other generic methods. Default-chain
arrays retain their existing fast paths, while Proxy prototypes keep their
dedicated trap handling. Fixes #9220 and #9221.

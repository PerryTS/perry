Reading a missing static off a built-in constructor that owns an accessor (for example
`Array.isBuffer`, since `Array` owns `Symbol.species`) no longer falls back to the slow
by-name lookup: the layout probe continues at `Function.prototype` once the function's own
properties prove the key absent. A `recv.constructor.isBuffer` loop over arrays runs about
17x faster (#12720).

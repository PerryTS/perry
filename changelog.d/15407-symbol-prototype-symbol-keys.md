`Symbol.prototype` owns `Symbol.toStringTag` (`"Symbol"`) and `Symbol.toPrimitive`
(a brand-checking function of length 1), both non-writable and configurable (#15244).

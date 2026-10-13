`Object.prototype.hasOwnProperty.call(receiver, key)` now reads the live
`Object.prototype.hasOwnProperty` method, so replacing the method is respected.
This is a partial fix for #11915; other method-call rewrite cases remain open.

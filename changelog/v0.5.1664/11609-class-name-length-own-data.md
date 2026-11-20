### Fixed

A class constructor's `name` and `length` are now own data properties of its
function object, as in Node: `delete C.name` makes `C.name` read the value
inherited from its prototype (`""` from `Function.prototype`, or the parent
class's name for a subclass) instead of `undefined`, `delete C.length` reads
`0`, and `Object.getOwnPropertyNames(C)` no longer lists a deleted `name` or
`length`. Reading `C.name` or `obj.constructor.name` no longer builds a new
string on every read (about 5x fewer instructions for `C.name`).

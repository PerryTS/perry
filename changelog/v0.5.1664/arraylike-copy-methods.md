`Array.prototype.flatMap`, `flat`, `toReversed`, `toSorted`, `with`, `keys` and
`entries` work on a Proxy over an Array (a Solid store array) and on a plain
array-like receiver (`Array.prototype.flatMap.call(obj, f)`). On a Proxy they
returned `{}` / `[]` or threw "value is not iterable"; borrowed onto an
array-like they threw "flatMap is not a function". Each now runs the
spec-generic body over the receiver's `[[Get]]` / `[[HasProperty]]`, so Proxy
traps fire in specification order. opencode's session view crashed on the
first prompt ("Cannot read properties of undefined (reading 'length')"): its
message list is a Solid store, and `flatMap` over it returned an object.

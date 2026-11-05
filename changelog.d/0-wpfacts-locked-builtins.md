Whole-program facts read the built-in prototype lock (decision 250). A store
whose target is unknown (a value from code the compiler does not see) can no
longer be taken to replace a member of a locked built-in prototype or to add a
key to `Object.prototype`: the runtime refuses both with a `TypeError`. Calls
such as `[1, 2].push(3)`, `s.indexOf(x)` or `map.get(k)` stay calls of the
built-in and keep their receivers after such a store. Reads the lock leaves
open (`constructor`, a new key on `Map.prototype`, a new named key on
`Array.prototype`, function, iterator and error prototypes) still see it.
An accessor defined under a computed key on a value that may be
`Object.prototype` now reaches an object whose prototype is
`Object.prototype` only through a computed key or `constructor`, so plain
objects whose missing-key reads are named keep their layout facts.

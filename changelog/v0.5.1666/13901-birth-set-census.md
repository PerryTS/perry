`o.k = v` after `const o = {}` and `this.k = v` in a constructor whose field
is only declared were turned into a plain field write at the object's birth
even when a prototype defines a setter or a read-only `k` (through an alias of
`Object.prototype`, `new Function`/`eval`, a relinked prototype, or another
module). Both folds now ask the program's prototype-write census, which
already decides this for `Object.create` families: a key folds only when no
prototype can intercept its add; otherwise the assignment stays a `[[Set]]`.

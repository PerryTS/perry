Adding a key to `Object.prototype`, `Function.prototype`, `Iterator.prototype`
or the array iterator prototype now throws a `TypeError` in a compiled program
(decision 250, extension (i)), and so does adding an array index to
`Array.prototype` (`Array.prototype[5] = 1`); named members may still be added
to `Array.prototype` and to every other built-in prototype. Compiled code relies
on those keys being absent: a plain object is not iterable, an iterator has no
`return` unless it defines one, an array hole reads `undefined`.

Compatibility: a polyfill that adds a method to `Object.prototype` or
`Function.prototype` must move it to a helper function. The global bindings
(`Map`, `Date`, `Promise`, …) stay patchable, and so do typed arrays, `TypeError`
and `URL` prototypes, which are outside the lock set.

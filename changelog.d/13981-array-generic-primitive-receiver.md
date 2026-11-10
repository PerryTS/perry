`Array.prototype` methods called on a primitive receiver through `.call`, `.apply` or a
held method value (`Array.prototype.slice.call("abc", 1)`, `const push = Array.prototype.push;
push.call(true, 1)`) again run on `ToObject(this)`: `slice` of a string returns its characters
(was `[]`, and a number receiver crashed), `push`/`pop`/`shift`/`unshift`/`reverse`/`splice`
throw on a string and mutate the Number or Boolean wrapper (were `undefined`), and
`keys`/`values`/`entries` iterate the wrapper and throw on `null`/`undefined`. The method value
converts a non-object receiver at its entry; object receivers keep their existing paths.

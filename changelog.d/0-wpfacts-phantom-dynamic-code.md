Whole-program facts no longer conclude that a program may run `Function(...)`
when it reads a function out of an object under a computed key
(`const fn = table[node.kind]; fn(node)`). The facts collapse the builtin
prototypes into one object, so the `constructor` found on a plain object
literal's chain was "Object or Function", and calling it counted as dynamic
code: every argument escaped, and so did everything reachable from it. On
TypeScript's checker that put 17,274 sites behind "E14 dynamic code",
although TypeScript runs no `Function` or `eval` at all.

The `constructor` on a prototype chain is now the one that chain has: only a
function's chain reaches `Function.prototype`. An object literal's is
`Object`, an array's is `Array`. A builtin prototype that the program sets as
a value (`__proto__`, `Object.setPrototypeOf`, `Object.create`) may still be
`Function.prototype`. A computed key on an object literal also reads
`Object.prototype`'s methods. A call of such a value runs each method's
effect on its receiver: `valueOf` returns it, and `__defineGetter__` with a
function installs an accessor on it.

Two related gaps closed: a builtin constructor's own `constructor`
(`Object.constructor`, `Array[k]`) is the Function constructor. A native
prototype's `constructor` is its native class, not `Function`.

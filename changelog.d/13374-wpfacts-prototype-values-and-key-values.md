Whole-program facts: a prototype taken as a value is now the prototype the
runtime has. `o.__proto__`, `o["__proto__"]`, `Object.getPrototypeOf(o)` and
`Reflect.get(o, "__proto__")` read the receiver's prototype (a literal's is
`Object.prototype`, a class instance's its class prototype); they used to read
nothing, or the literal's internal layout, so a store through them was missed.
A computed key is still never taken to be `"__proto__"`. A number key now
reads a class prototype's numeric members (`d[n]` missed `C.prototype[0]`),
and a computed call with a number key (`table[node.kind](node, cb)`,
`arr[i](...)`) calls the target's own and program members, never an
`Object.prototype` method such as `__defineGetter__`. Per unit, a
`defineProperty` / `delete` on `Object.prototype` in one unit now makes every
chain through it HARD in the other units too, as the whole program does, and
another unit's computed-key accessor on `Object.prototype` no longer turns
every fact off.

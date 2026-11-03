`obj.toLocaleString()` on an ordinary object whose prototype chain defines
`toLocaleString` (a function constructor's `prototype`, a replaced or
`Object.create` prototype) printed `[object Object]`: the by-name dispatcher
answered `Object.prototype.toLocaleString` without reading the receiver's
chain. It now reads the method off the chain like `valueOf` and
`hasOwnProperty` already do; `Object.prototype`'s own method still answers
when nothing earlier on the chain defines one.

Whole-program facts no longer model a function's `arguments` object as an
Array. It is an ordinary object whose prototype is `Object.prototype`, with
own data `length` and `callee`: a store or `Object.defineProperty` on
`arguments.length` now keeps the stored value, where the facts used to say
it was always a Number. Proven site counts are unchanged on the chess
witness, tssrc, tscwork, richards and fastify_inject.

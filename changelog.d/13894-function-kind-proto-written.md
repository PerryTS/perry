A generator, async or async-generator function called through `apply`,
`call` or `bind` by name now reads the method off its kind prototype once
program code wrote that prototype (`Object.getPrototypeOf(gen).apply = f`, an
accessor, a relink). It used to answer the builtin `Function.prototype`
method regardless. Unwritten kind prototypes keep the builtin answer.

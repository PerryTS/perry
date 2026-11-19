An ordinary function constructor's run of `this.k = v` stores (React's
`FiberNode`) checks its receiver once (`perry_function_run_admits`) and then
writes each key's slot in place, as a class constructor's run does, instead
of proving every store's add through the prototype chain. Only a function the
module constructs by name arms a run, so a method-style function pays
nothing. The React 19 keyed-list switch of #14974 runs about 21% faster.

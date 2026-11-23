On `--target web`, `for...in` over a string primitive visits its indices again: the wasm emitter compiles `Object(value)` instead of yielding `undefined`, which the for-in key recheck relies on.

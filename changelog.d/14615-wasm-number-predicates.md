`--target web`: `Number.isNaN`, `Number.isFinite`, `Number.isInteger` and
`Number.isSafeInteger` returned `undefined`, because the WASM emitter had no
arm for them. They now answer as in Node, without coercing their argument.

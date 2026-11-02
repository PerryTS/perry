An arrow created in a derived constructor before `super()` reads `this`
through the binding cell that `super()` fills. Reading it before `super()`
returns now throws the same `ReferenceError` as any other `this` access
before `super()`, instead of evaluating to `undefined`.

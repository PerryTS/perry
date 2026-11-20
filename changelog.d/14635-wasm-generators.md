`--target web`: generators returned `undefined` from `next()`, because the web
pipeline skipped the generator transform and the wasm backend turned `yield`
into `undefined`. Sync generators now lower to the same state machine as on
native targets, and `next`, `return` and `throw` (with `finally`) behave as in
Node. `for...of` and destructuring over a generator are not covered yet.

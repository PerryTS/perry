`--target web`: `new Set([...])`, `new Map<K, V>([...])` and a typed collection's `set`/`get`/`has`/`delete`/`clear` work inside an `async` function instead of evaluating to `undefined` (#14595).

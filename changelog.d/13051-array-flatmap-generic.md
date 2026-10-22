`Array.prototype.flatMap` has a generic engine (`js_arraylike_flatMap`) and a
real prototype thunk. A call on a Proxy over an array or on an array-like now
returns a flattened array; before, the noop thunk returned a non-array, and
the compiled opencode TUI threw "map is not a function" on every keystroke.
`flatMap.call/apply` and an explicit `thisArg` on array receivers go through
the same engine. Gap test: `test_gap_flatmap_proxy_receiver.ts`.

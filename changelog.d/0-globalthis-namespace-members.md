A program that uses `globalThis` (or another global-object alias) as a value
now gets complete `Math`, `Reflect`, `Atomics` and `Intl` namespaces and the
`TextEncoder`/`TextDecoder` and `WebSocket` prototype members when it reads
them by a runtime key (`globalThis[k]`). They used to be born member-less
(`Math.abs` was `undefined`), because the global-object escape forced only
the JSON and URL member tables on.

A sloppy function that is only ever called with `new` (every reference to it
is a `new F(...)` callee) now has a fresh `this`, so handing that `this` to
other code no longer counts as the global object escaping. Programs such as
the awfy `havlak` benchmark, whose `BasicBlockEdge` passes `this` to
`cfg.addEdge(this)`, no longer switch every namespace and web member table
on at startup.

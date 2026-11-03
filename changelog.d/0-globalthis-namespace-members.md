A program that uses `globalThis` (or another global-object alias) as a value
now gets complete `Math`, `Reflect`, `Atomics` and `Intl` namespaces and the
`TextEncoder`/`TextDecoder` and `WebSocket` prototype members when it reads
them by a runtime key (`globalThis[k]`). They used to be born member-less
(`Math.abs` was `undefined`), because the global-object escape forced only
the JSON and URL member tables on.

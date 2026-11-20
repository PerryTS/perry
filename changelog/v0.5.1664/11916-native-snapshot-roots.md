Fix stale addresses in native stream and promise code when user code or an
allocation collects mid-call: classic `stream.pipeline` (stream and
function-stage paths), `stream.compose`, `Duplex.from`, `stream.finished`,
`duplexPair`, `readable.read()`/`read(n)`, the readable iterator helpers
(`map`, `filter`, `flatMap`, `take`, `drop`, `toArray`, `forEach`, `reduce`,
`find`, `some`, `every`), `Promise.all`/`race`/`allSettled`/`any`,
`Promise.resolve` with a thenable, and `fs/promises` results.
`test_parity_stream` no longer crashes under the seeded moving GC schedule.

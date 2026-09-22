Fix a cluster of four `net.Socket` gaps the package audit hit while compiling
real socket-backed npm packages (mysql2, pg, redis, ws) natively: missing
`prependListener`/`prependOnceListener` (#10441), `on()`/`addListener()`
returning `undefined` on a typed `net.Socket` receiver instead of the socket,
breaking `.on(...).on(...)` chaining (#10442), a missing `pipe()` (#10444),
and missing/incorrect `writable`/`readable`/`readyState`/`connecting`/
`pending`/`destroyed`/`_writableState`/`_readableState` (#10465).

Root cause was shared shape (an incomplete dispatch table, both the untyped
dynamic-dispatch path and the typed `net.Socket` codegen table), but not a
single shared fix: #10441/#10442 were table-completion, #10444 needed a new
`pipe()` implementation (`crates/perry-ext-net/src/pipe.rs`, via the same
generic `Get("write")`+call duck-typed dispatch the runtime already uses for
thenables), and #10465 needed new lifecycle state tracking
(`SocketState::connecting`/`writable_ended`/`readable_ended`/`has_opened`).

Validating #10465 against Node byte-for-byte surfaced two additional bugs,
fixed here: `destroyed`/`is_open` were flipped on the tokio task thread as
soon as teardown started, before the main thread had processed the `'end'`
event that same teardown queued, so a `pending`/`destroyed` read from inside
an `'end'` listener disagreed with Node; and `pipe()`'s own route-tracking
table cached a socket-destination pointer outside every GC root scanner,
which a copying GC cycle between `pipe()` and `unpipe()` could turn stale.

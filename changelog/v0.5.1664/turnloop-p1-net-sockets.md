turnloop P1: `node:net`'s listeners, accepted connections and local (Unix-domain
socket / Windows named pipe) sockets now live on the primary agent's
`turnloop::Loop` instead of on tokio. One multishot `accept_start` replaces the
`spawn_async` accept loop each listening server used to pin, one multishot
`read_start` replaces the per-connection `run_socket_task`, and `write`/`end()`/
`destroy()` submit to the driver at the FFI call site instead of travelling
through a per-socket `mpsc` command channel. `socket.bytesWritten` and the
queued-byte count behind `write()`'s boolean return are now the driver's own
accounting rather than a hand-maintained tally.

The JS surface is unchanged: the same `PendingNetEvent`s reach the same queue in
the same order and are drained by the same pump, so listener maps, the GC root
scanner, the read buffer pool and every `net.Socket` property behave as before.

Outbound TCP clients stay on tokio in this phase. `socket.upgradeToTLS` hands a
live `TcpStream` to `tokio_rustls` mid-stream, turnloop owns its descriptor
without exposing it, and a socket's transport is fixed at creation — so the
class that can be upgraded stays where the upgrade works. Local sockets move
because that upgrade already refused them, and a `worker_threads` agent keeps
tokio because it has no loop until P3/P4.

New in the runtime: `perry-runtime/src/turnloop_net`, the loop-owned socket
layer (handle table, token-encoded completion routing, Node `code`/`errno`/
`syscall` mapping, write backpressure, half-close, ref/unref) plus the C ABI a
separately linked binding uses to reach it, wrapped safely in
`perry-ffi::turnloop_net`. Reads land in turnloop's pooled buffers and writes
are handed over as owned `Vec`s, so no JS heap memory ever reaches the driver
and the I/O path needs no GC root scanner. Client connects walk the whole
resolved address list (Node's `autoSelectFamily`), with the name lookup on the
shared blocking pool rather than the event-loop thread.

`PERRY_LOOP_STATS=1` gains a `completions=` counter, so a run can prove turnloop
actually carried the I/O rather than merely that a turn happened.

turnloop is now `0.1.0-alpha.3`, whose caret dependency requirements lift the
workspace-wide lockfile downgrades alpha.2 forced (libc, tokio, redis and the
wasm-bindgen family are back at the versions `main` resolved before P0).

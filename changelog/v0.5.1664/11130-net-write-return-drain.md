**`net.Socket#write()` returns Node's boolean and emits `'drain'`; a write burst can no longer exhaust the event loop (#11111, #11106).**

- **#11111:** `socket.write()` returned `undefined`, so drain-aware writers hung. mongodb 7.5.0's `Connection.writeCommand` awaits `'drain'` on any falsy return, so its first `hello` never completed.
  - `write()` now returns `writableLength < writableHighWaterMark` (64 KiB), counting the chunk first, as Node's `Writable.prototype.write` does.
  - A `false` return sets `writableNeedDrain`, and exactly one `'drain'` follows when the queue empties. It is emitted before the completed write's callback, and never while ending or destroyed.
  - A write to an ended or destroyed socket returns `false`.
  - `writableLength`, `writableHighWaterMark` and `writableNeedDrain` are now exposed on the untyped dispatch path. `bufferSize` reports the real queue length.
  - Changed sites: the codegen `Socket.write` signature (`NR_VOID` → `NR_F64`), the ext-net handle dispatch, and perry-stdlib's `fastify_net_zlib` arm.
- **#11106:** 40,000 × 64-byte writes followed by `end()` delivered 0 bytes.
  - Root cause: every `write()` was its own turnloop operation, and the loop's shared operation table holds 32,768. The next submission failed with `write ENOMEM`, and the resulting destroy cancelled every queued byte.
  - Fix: `perry-runtime/src/turnloop_net/write_queue.rs` keeps at most one driver write in flight per socket, like libuv's one active write request per stream. Later writes, and an `end()` behind them, collect in a per-socket backlog that is submitted as one buffer when the in-flight write completes.
  - Completions are still reported once per `write()`, in order, with their own token and length.
- **Pre-connect writes:** the backlog also holds writes issued before `connect` completes. A write to a socket still resolving `localhost` used to fail with `write ENOENT` and hang. It now reaches the connection that succeeds.

Tests:
- Gap tests: `test_gap_net_socket_write_return_drain`, `test_gap_net_write_burst_then_end` and `test_gap_net_write_before_connect_hostname`. All are byte-identical to Node 26.5.1. They fail on the #11105 base and pass with this change.
- Unit tests in `perry-ext-net` (`lifecycle.rs`) and `perry-runtime` (`turnloop_net/tests.rs`). The runtime test drives 40,000 writes through the real driver.

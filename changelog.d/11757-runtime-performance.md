- Implement nested `net.Socket.cork()` / `uncork()` buffering and expose `writableCorked` through typed and dynamic dispatch. Flush corked bytes as one transport write on the final `uncork()` or `end()`, preserve write-callback order and GC custody, and include buffered bytes in backpressure accounting. Propagate failed end-time flushes after releasing the socket registry lock.
- Resolve private interpreter scope bindings directly through their data slots, including undefined bindings and overflow storage. Retain dynamic object-environment behavior for VM contexts, and reuse rooted key strings for global presence checks.
- Avoid scanning discarded tails for limited literal string splits and reuse the receiver when the separator does not match.
- Add Node parity coverage for deferred socket writes, nested corking, unopened corked sockets, ordered destroy-time callbacks and pre-connect cancellation cleanup, plus checksum-checked reproductions of #10519, #10525, #10526 and #10528.
- Classify the cork and uncork providers as registry-handle results in the checked native-result ledger.

Keep the generated WASM runtime ABI table current for the new socket `cork`, `uncork`, and `writableCorked` exports, preserving their native source signatures.

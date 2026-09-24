**net / tls: CodeRabbit follow-ups to #11130.**

- `write()` on a TLS socket now judges backpressure against outstanding application bytes, not the ciphertext queue. The ciphertext queue is empty mid-handshake, so a 64 KiB write before `'secureConnect'` returned `true`.
- Writes and `end()` to a `tls.connect` socket made before its TCP connect completed are held and replayed through the TLS layer. They reached the wire unencrypted ahead of the ClientHello, and the server failed the handshake (`DecodeError`).
- A write or `end()` while a connect plan is between attempts is queued for the next attempt instead of being refused, which errored and destroyed the socket.
- Repeated `end(cb)` calls all run their callbacks, in order (only one shutdown token was kept).
- `write()` on `new net.Socket()` before `connect()` returns `false`, as in Node.

Tests: gap tests `test_gap_tls_write_backpressure_handshake` and `test_gap_net_socket_end_twice_callbacks`, plus unit tests in `perry-runtime` (`turnloop_net`) and `perry-ext-net` (`lifecycle`).

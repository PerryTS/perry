### Fixed

`net.Socket.read()` now implements the readable-stream pull contract for both
bundled and optimized net providers. Pull-mode consumers such as undici receive
queued bytes as `Buffer` values and `null` when the socket queue is empty,
instead of falling through to `undefined`.

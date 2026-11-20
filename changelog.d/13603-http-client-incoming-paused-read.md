The client `IncomingMessage` of `node:http` and `node:https` had no paused-mode
Readable surface, so draining a response with `on("readable")` and `read()`
(as Effect's `NodeStream.fromReadable` does) spun on an undefined `res.read`
and never saw the body. A `readable` listener now buffers the body, `read([size])`
returns a Buffer, a string after `setEncoding`, or `null`, and `end` follows only
once the buffer is drained, then `close`. `pause()` and `resume()` hold back and
flush `data` listeners and pipes, `destroy([err])` ends the exchange, and
`removeListener`, `readable`, `readableEnded`, `closed` and `destroyed` follow Node.

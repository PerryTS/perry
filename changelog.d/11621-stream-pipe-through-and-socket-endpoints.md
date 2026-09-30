Fix `ReadableStream.pipeThrough()` with object-backed native transforms such as
`DecompressionStream`, `CompressionStream`, and `TextEncoderStream`, and with
user-supplied readable/writable pairs. Resolve endpoints from existing object
fields while retaining the numeric TransformStream path.

Restore `remoteAddress`, `remotePort`, `remoteFamily`, `localAddress`, `localPort`,
and `localFamily` on accepted and upgraded sockets accessed through untyped
receivers. Serve the existing socket endpoint state through dynamic dispatch;
no new state, caches, or side tables are introduced.

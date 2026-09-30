Fix `ReadableStream.pipeThrough()` with object-backed native transforms such as
`DecompressionStream`, `CompressionStream`, and `TextEncoderStream`, and with
user-supplied readable/writable pairs. Resolve endpoints from existing object
fields while retaining the numeric TransformStream path.

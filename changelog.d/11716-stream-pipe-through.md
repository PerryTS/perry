Fix `ReadableStream.pipeThrough()` with object-backed native transforms such as
`DecompressionStream`, `CompressionStream`, and `TextEncoderStream`, and with
user-supplied readable/writable pairs. Resolve endpoints from existing object
fields while retaining the numeric TransformStream path.

Keep the object-pair helpers in a cold executable code section with valid unwind
metadata on macOS, so invalid endpoints and throwing getters reach JavaScript
catch handlers and release their temporary GC roots.

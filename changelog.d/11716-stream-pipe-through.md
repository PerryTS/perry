Fix `ReadableStream.pipeThrough()` with object-backed native transforms such as
`DecompressionStream`, `CompressionStream`, and `TextEncoderStream`, and with
user-supplied readable/writable pairs. Resolve endpoints from existing object
fields while retaining the numeric TransformStream path.

Keep object-pair conversion in cold functions using standard platform code
sections and unwind metadata, so invalid endpoints and throwing getters reach JavaScript
catch handlers and release their temporary GC roots.

Borrow the caller’s scoped receiver root during pair unwrapping, reuse it when an object-backed transform pair unwraps to itself, and cover self-aliasing and distinct hidden-handle getters under forced moving collection.

Avoid repeated numeric range and integer conversion checks for non-object pairs with default options; preserve endpoint validation and rejection of invalid numeric and tagged primitive arguments.

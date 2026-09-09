### Performance

- Accelerate `JSON.parse` and `JSON.stringify` across tiny values, small records, and multi-megabyte documents while keeping construction isolated from GC tracing. Parsed results remain fresh, mutable values; immutable source-derived data is reused through bounded caches, and large JSON strings use individually tracked leaves.

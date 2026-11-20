Fixed `Uint8Array`/`Buffer.prototype.set(source, offset)` paying a view-registry
lookup (and, for `Buffer`/`TypedArray` sources, an extra `Vec<u8>` copy) per
byte instead of per call, reaching 171x Node at 100k bytes and 82x at 1M
(#10088). `js_buffer_set_from_value` now resolves a `Buffer` source's view
indirection once via `view::resolve_data_ptr` (already the codebase's
established span resolver, used by `bun_ffi`/`DataView`) and, for a
same-element-width (1-byte) `TypedArray` source (`Int8Array`, `Uint8Array`,
`Uint8ClampedArray`), reads its raw byte span directly via
`typedarray::data_ptr` — the stored byte already equals what `to_uint8` of the
read element would give for all three kinds. The single resulting span is then
copied straight into the target with `ptr::copy` (a memmove), which also
correctly handles the case where the resolved source span physically overlaps
the destination (e.g. `buf.set(buf.subarray(2), 0)`) without needing an
intermediate buffer. `Array`/`Object` sources and wider or BigInt-kind
`TypedArray` sources are untouched — they still need per-element
coercion/property reads. 1M-byte `Uint8Array.set` measured ~1.2x Node locally,
down from the issue's ~82x.

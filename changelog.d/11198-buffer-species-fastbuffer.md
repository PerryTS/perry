Add `Buffer[Symbol.species]`, Node's internal `FastBuffer`: an own
`get [Symbol.species]` accessor on `Buffer` (`enumerable: false`,
`configurable: true`) returning a constructor that shares `Buffer.prototype`.
`new FastBuffer(arrayBuffer, byteOffset, length)` is a Buffer view over that
memory and `new FastBuffer(size)` is zero-filled. Perry answered `undefined`.
The code is in
`perry-runtime/src/object/native_module/callable_exports/buffer_species.rs`.
`FastBuffer` is held in the getter closure's capture slot, so no new runtime
root holder is added.

undici 8.9.0 builds every llhttp callback argument with
`new (Buffer[Symbol.species])(...)`, so its first response status line threw
"undefined is not a constructor" (part of #11046). This covers only the Buffer
half of #11193; the generic `get [Symbol.species]` on `Array`, `Map`, `Set`,
`Promise`, `RegExp`, `ArrayBuffer` and `%TypedArray%` is still missing.
Covered by `test-files/test_gap_11193_buffer_species_fastbuffer.ts`.

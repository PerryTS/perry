**perf(codegen): an untyped `b[i] = v` stores a byte inline into an admitted Uint8Array (#10515)**

nanoid's pool refill through untyped parameters (`b[i] = cc[b[i] & mask]`,
plain `.js`) cost about 678 instructions per element against node's 16
(42x). The reads already had an inline arm keyed on the receiver's admission
in `PERRY_U8_INLINE_CACHE` (a live, owning, inline-storage byte view); the
store did not, so every element called `js_dyn_index_set_strict`, which walks
the proxy / symbol / typed-array / prototype / temporal / class-value ladder
before reaching the same cached byte store, and computed `ToUint8` with
`fmod`.

The dynamic store's fall-back now first tests that admission, an exact
integer index below the buffer's length, and a plain-number value in
`(-2^31, 2^31)`, and stores the low byte of its truncation (which is
`ToUint8`). Every other receiver, key or value (NaN, infinities, larger
magnitudes, strings, objects with `valueOf`, clamped arrays, views) still
takes the complete `[[Set]]`.

u8any: 22,219,440 -> 6,195,360 instructions per 32 KiB refill (42x -> 11.6x
node). The rest is the untyped element reads' dispatch in the compiled loop.

The polymorphic index read (a captured Uint8Array read, the nanoid closure
shape) answers an admitted Uint8Array from the same byte cache before its
receiver-classification ladder: nanoid 14,616,106 -> 8,291,381 instructions
per 32 KiB refill.

Test: `test-files/test_gap_10515_u8_untyped_store.ts`.

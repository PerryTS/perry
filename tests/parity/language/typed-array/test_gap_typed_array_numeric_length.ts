// Gap test: multi-byte numeric-length typed-array .length / .byteLength
// Run: node --experimental-strip-types test_gap_typed_array_numeric_length.ts
//
// Regression coverage for the #1862 typed-view rewrite, which began
// registering `new Int32Array(N)` / `new Float64Array(N)` (multi-byte,
// numeric-length, non-mutated) bindings in `buffer_data_slots`. The Buffer
// `.length` fast path hardcoded a `data-8` header read (valid for an 8-byte
// BufferHeader / Uint8Array) but a TypedArrayHeader is 16 bytes, so it read the
// packed `kind|elem_size` bytes instead: Int32 -> 1028 (0x404),
// Float64 -> 2055 (0x807), regardless of N. `.byteLength` returned undefined
// because the generic runtime getter had no typed-array branch. These bindings
// are intentionally NOT mutated before the read so the buffer-view fast path
// fires (mutation disables it).

const d = new Int32Array(5);
console.log("Int32Array(5):", d.length, "byteLength:", d.byteLength, "d[0]:", d[0], "d[4]:", d[4]);

const e = new Float64Array(3);
console.log("Float64Array(3):", e.length, "byteLength:", e.byteLength);

const f = new Uint8Array(10);
console.log("Uint8Array(10):", f.length, "byteLength:", f.byteLength);

const g = new Int32Array(0);
console.log("Int32Array(0):", g.length, "byteLength:", g.byteLength);

const h = new Int32Array([1, 2, 3]);
console.log("Int32Array([1,2,3]):", h.length, "byteLength:", h.byteLength);

const i16 = new Int16Array(4);
console.log("Int16Array(4):", i16.length, "byteLength:", i16.byteLength);

const u32 = new Uint32Array(6);
console.log("Uint32Array(6):", u32.length, "byteLength:", u32.byteLength);

const f32 = new Float32Array(7);
console.log("Float32Array(7):", f32.length, "byteLength:", f32.byteLength);

// BYTES_PER_ELEMENT instance property
console.log("Int32 BYTES_PER_ELEMENT:", d.BYTES_PER_ELEMENT);
console.log("Float64 BYTES_PER_ELEMENT:", e.BYTES_PER_ELEMENT);

// byteOffset of a freshly-constructed view is 0
console.log("Int32 byteOffset:", d.byteOffset);

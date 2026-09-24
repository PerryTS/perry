Fix Node `Buffer` failing every typed-array/view predicate, and make all of those predicates answer from one classifier (#11239).

`ArrayBuffer.isView(buf)` (direct call and `const f = ArrayBuffer.isView`), `util.types.isArrayBufferView/isTypedArray/isUint8Array` all returned `false` for a Buffer from any constructor (`alloc`, `allocUnsafe(Slow)`, `from(...)`, `concat`, `slice`, `subarray`, `new FastBuffer(...)`). bson's `UUID` constructor gates on `ArrayBuffer.isView(input) && input.byteLength === 16`, so decoding any stored UUID threw, and mongodb's `listCollections` failed on every collection's `info.uuid`.

Root cause: five JS types share `BufferHeader` storage and the buffer registry (Buffer, Uint8Array, ArrayBuffer/SharedArrayBuffer, DataView, crypto key material), and each predicate spelled its own subset of the side-table probes. `isView` and `util.types` only recognised constructor-made `Uint8Array`s; `instanceof Uint8Array`/`instanceof Buffer` tested bare registry membership, which also holds for an ArrayBuffer, a DataView and key material.

Fix:
- `buffer::buffer_brand(addr) -> Option<BufferBrand>` (`buffer/exotic_view.rs`) classifies a registered buffer once. `is_node_buffer` (`Buffer.isBuffer`) now derives from it. So does `is_uint8_view_buffer`, which backs `is_typed_array_buffer` (the `%TypedArray%.prototype` receiver gate) and keeps exactly its old probe set.
- `object::view_brand::view_brand(value) -> Option<ViewBrand>` (new `object/view_brand.rs`) layers the typed-array registry and user subclasses (`class X extends DataView/Uint8Array`) on top. `ArrayBuffer.isView` (both forms), every `util.types.is*Array`, `isArrayBufferView`, `isTypedArray`, `isDataView`, and `instanceof Uint8Array` / `instanceof Buffer` read it.
- `instanceof Buffer` gets its own reserved class id (`NODE_BUFFER_CLASS_ID = 0xFFFF000C`) in codegen and the dynamic-RHS path. It used to share `Uint8Array`'s id, so `new Uint8Array(1) instanceof Buffer` was `true`.
- `Object.getPrototypeOf(Buffer) === Uint8Array` now holds, so inherited statics resolve (`Buffer.BYTES_PER_ELEMENT === 1`).

Other defects this fixes on main: `instanceof Buffer` was true for a DataView, an ArrayBuffer and a plain Uint8Array (static and dynamic RHS); `new DataView(ab) instanceof Uint8Array` and `new ArrayBuffer(n) instanceof Uint8Array` were true; `class S extends Uint8Array` instances were not `instanceof Uint8Array`.

Test: `test-files/test_gap_buffer_view_predicate_matrix.ts`, 18 predicates × 31 values plus prototype-chain and dynamic-`instanceof` rows. Byte-identical to Node 26.5.1 with the fix; 29 lines differ on main. Unit tests are in `object/view_brand_tests.rs`.

Left for separate fixes (found during the audit): `class S extends DataView` instances report `constructor.name` `"DataView"`; `Object.create(X.prototype) instanceof X` is false for every reserved-id builtin (`Map`, `Uint8Array`, …), so `Buffer.prototype instanceof Uint8Array` is false; `typeof Buffer.from` is `"undefined"` although `Buffer.from(...)` works.

Follow-up to #11239 (#11243 fixed the four `isView`/`util.types` predicates for Buffers): every typed-array/view predicate now answers from one classifier, which also fixes the `instanceof Buffer` / `instanceof Uint8Array` misclassifications that audit found.

Root cause: five JS types share `BufferHeader` storage and the buffer registry (Buffer, Uint8Array, ArrayBuffer/SharedArrayBuffer, DataView, crypto key material), and each predicate spelled its own subset of the side-table probes. `instanceof Uint8Array` and `instanceof Buffer` shared one reserved class id and tested bare registry membership, which also holds for an ArrayBuffer, a DataView and key material.

Fix:
- `buffer::buffer_brand(addr) -> Option<BufferBrand>` (`buffer/exotic_view.rs`) classifies a registered buffer once. `is_node_buffer` (`Buffer.isBuffer`) derives from it, and so does `is_uint8_view_buffer`, which backs `is_typed_array_buffer` (the `%TypedArray%.prototype` receiver gate, keeping exactly its old probe set).
- `object::view_brand::view_brand(value) -> Option<ViewBrand>` (new `object/view_brand.rs`) layers the typed-array registry and user subclasses on top. `ArrayBuffer.isView` (direct and as a value), every `util.types.is*Array`, `isArrayBufferView`, `isTypedArray`, `isDataView`, and `instanceof Uint8Array` / `instanceof Buffer` all read it. #11243's per-call-site `is_typed_array_buffer` checks and the duplicated `jsvalue_extends_*` helpers are folded into it; #11243's unit and gap tests are kept and pass.
- `instanceof Buffer` gets its own reserved class id (`NODE_BUFFER_CLASS_ID = 0xFFFF000C`) in codegen and on the dynamic-RHS path. It used to share `Uint8Array`'s id, so `new Uint8Array(1) instanceof Buffer` was `true`.
- `Object.getPrototypeOf(Buffer) === Uint8Array` now holds, so inherited statics resolve (`Buffer.BYTES_PER_ELEMENT === 1`).

Defects fixed on main: `instanceof Buffer` was true for a plain Uint8Array, a DataView, an ArrayBuffer and a SharedArrayBuffer (static and dynamic RHS); `instanceof Uint8Array` was true for a DataView and an ArrayBuffer; `class S extends Uint8Array` instances were not `instanceof Uint8Array`; `Object.getPrototypeOf(Buffer) === Uint8Array` was false.

Test: `test-files/test_gap_buffer_view_predicate_matrix.ts`, 18 predicates × 31 values plus prototype-chain and dynamic-`instanceof` rows. Byte-identical to Node 26.5.1 with the fix; fails on main. Unit tests are in `object/view_brand_tests.rs`.

Found during the audit and filed separately: #11255 (`class extends DataView` `constructor.name`), #11256 (`Object.create(X.prototype) instanceof X` for reserved-id builtins), #11257 (`typeof Buffer.from` is `"undefined"`), #11259 (class `.name` mangled through a re-exported namespace, affecting every bson `constructor.name`).

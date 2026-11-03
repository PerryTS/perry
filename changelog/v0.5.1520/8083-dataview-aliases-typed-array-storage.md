### Fixed

- **runtime: a `DataView` did not see writes made through a multi-byte typed array over the same `ArrayBuffer`.** `new Uint32Array(ab)` followed by `w[0] = 0x01020304` left `new DataView(ab)` reading the bytes that were there when the DataView was *constructed* — `dv.getUint8(0..3)` summed to `0` where node says `10`. No throw, no null: just numbers from before the write. This is the DataView half of the #7219 aliasing family; the typed-array-to-typed-array half shipped as `test_gap_typedarray_buffer_aliasing_7219.ts`.

  **Root cause: the DataView read its own snapshot, not the backing store.** `js_data_view_new` (`buffer/from.rs:894`) allocates the view its own `BufferHeader`, copies the window's bytes in, and registers it in the buffer view registry. That local copy is refreshed only by writes that route *through* the registry — `js_buffer_set`, `js_buffer_write`, a sibling DataView's `set*`. A `Uint16Array`/`Uint32Array`/`Float64Array` element store does not: `typedarray_view::register_view_meta` makes `typedarray::data_ptr_mut` resolve straight into the backing `ArrayBuffer`, and the store lands there with nothing mirroring it into the DataView. `read_bytes` in `buffer/dataview.rs` then read the DataView's inline bytes, so every `get*` returned the snapshot.

  **The fix is one line**: `read_bytes` resolves through `view::resolve_data_ptr`, the canonical view-resolving accessor `read_buffer_byte` has used for `Uint8Array`/`Buffer` receivers since #1205 and every native-span consumer since #6515. Writes were already correct — `write_bytes` mirrors into the backing via `propagate_written_range_from_receiver` — which is why the bug was direction-specific.

  Three things made it read like something else, and all three are now covered by tests:

  - **Element width looked like the trigger.** A `Uint8Array` writer worked, so `DataView` looked fine. It was not the width: a Perry `Uint8Array` over an `ArrayBuffer` is a `BufferHeader` view whose element writes go through `js_buffer_set`, which mirrors into every registered view. Only the kinds that get a `TypedArrayHeader` bypass the registry.
  - **Construction order looked like the trigger.** Writing before `new DataView(ab)` worked, because the constructor copies the bytes present at that moment.
  - **Buffer identity was already right.** `w.buffer === dv.buffer`, `byteOffset` and `byteLength` all reported correctly, which is why this reads as a correctness bug rather than a modelling one.

  The DataView keeps its own storage — the codegen path that `gep`s against a buffer pointer still needs it, and `write_bytes` still writes it so a `set*` is visible to anything reading the view's inline bytes. What changed is only which copy is *authoritative* on the read.

- **runtime: `TextDecoder.prototype.decode(dataView)` read the same stale snapshot.** Found while bounding the above, and fixed with it because it is the same line of reasoning: `js_text_decoder_decode_llvm` (`text.rs`) built its byte slice as `buf + sizeof(BufferHeader)` under a comment asserting the bytes are "stored inline". They are not, for a registered view. `decode(dv)` of an `ArrayBuffer` a `Uint32Array` had just written returned `"\0\0\0\0"` where node returns `"ABCD"`, while `decode(ab)` on the same buffer was correct — the tell that the receiver, not the bytes, was the problem. It now resolves through `buffer::resolve_span_data_ptr` like every other native-span consumer, which also fixes the `Buffer.from(ab)` / `subarray` receivers that share the branch.

### Testing

- `test-files/test_gap_dataview_buffer_aliasing_7219.ts` — byte-exact against `node --experimental-strip-types` 26.5.1 across every element width (`Uint8`/`Uint16`/`Int32`/`Float64`), both directions, DataView constructed before *and* after the writes, a windowed `new DataView(ab, 4, 8)`, `getInt16`/`getInt32`/`getFloat64` with and without the little-endian flag (DataView defaults to big-endian while a typed array is platform-endian — the asymmetry a byte-swapping "fix" would break), two DataViews over one buffer, a DataView over a typed array's lazily-materialized `.buffer`, module scope as well as function scope, and a loop-carried read/write mix. At base, 8 of its 11 lines were wrong.
- Three unit tests, all `cargo-test`-visible (per #5960) and all watched fail with their fix reverted: `data_view_reads_multi_byte_typed_array_writes` and `multi_byte_typed_array_reads_windowed_data_view_writes` in `crates/perry-runtime/src/buffer/mod.rs` (`DataView byte 0 lags the typed-array write: left 0.0, right 2.0`), and `text_decoder_reads_backing_store_of_a_data_view` in `crates/perry-runtime/src/text.rs` (`left "\0\0\0\0", right "ABCD"`). Each asserts the typed-array store actually reached the `ArrayBuffer` before comparing, so a store that never landed cannot pass it vacuously (`0 == 0`).

### Known limitations (unchanged by this change)

- A `DataView` over a **detached** buffer throws `RangeError`, where node throws `TypeError`. Detach zeroes every registered view's length (`buffer/detach.rs:76-83`), so the read is rejected as out-of-bounds before it can report the detach.
- **Resizable `ArrayBuffer`s are still unimplemented** (`ab.resize` is not a function), so a length-tracking DataView over one has nothing to track.

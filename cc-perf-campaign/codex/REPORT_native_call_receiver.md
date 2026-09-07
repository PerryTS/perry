# Native-call receiver registry cut

## Pushed code

- Branch: `fork/perf/native-call-receiver-class`
- Before-counter commit: `8ad6e0777c0104910c6db66e5e4b16862c95f6b0`
- Runtime implementation through: `72b713b99abdaf54b07a06b8fe29c177d2a47ff5`
- Base: `87dc33492`
- This is runtime-only. No codegen file changed. Re-measurement needs a relink against the I7-view tree's existing bundle cache; it does not need a bundle recompile.

## Source map and old need for the distinction

- `crates/perry-runtime/src/object/native_call_method.rs:1160`: `gc_pointer_and_type_from_value` returns `Option<(*const u8, u8)>`: the normalized receiver address plus its runtime `GC_TYPE_*`. It returns `None` for an invalid/unowned pointer and for Symbol, Map, Set, and RegExp cells, because those have dedicated dispatch paths.
- `crates/perry-runtime/src/object/native_call_method/handle_methods.rs:55`: native method dispatch needs Buffer versus TypedArray to select different method families. Buffer receivers enter `dispatch_buffer_method` (`object/buffer_dispatch.rs:371`), which owns Node Buffer numeric reads/writes, encoding/toString, compare/copy/fill/search/swap, slice/subarray, ArrayBuffer transfer, DataView accessors, and Buffer/ArrayBuffer/SAB-specific brands. TypedArray receivers enter `dispatch_typed_array_method` (`object/native_call_method/typed_array.rs:40`), which owns element-kind-aware `at`, callback methods, sort/reduce, set/subarray/slice, joins/searches, and the Array-only-method rejection behavior.
- Buffer, Uint8Array, ArrayBuffer, and SharedArrayBuffer cannot be flattened to one method-table answer. Perry represents ordinary managed Buffer/Uint8Array/ArrayBuffer cells with `GC_TYPE_BUFFER`, then preserves the finer brand registries inside buffer dispatch. Multi-byte typed arrays use `GC_TYPE_TYPED_ARRAY` plus `TypedArrayHeader.kind` (`typedarray/mod.rs:163`).
- The header tag alone cannot classify every valid receiver. `js_buffer_register_external` registers an embedder-owned `BufferHeader` with no Perry GC header (`buffer/header.rs:597`). `shared_sab::alloc_shared_sab` allocates the process-global SAB backing directly with `alloc_zeroed` and never gives it a GC header (`shared_sab.rs:60`). Native-arena typed views can likewise be headerless. These cases must retain registry fallback after an allocator-tracked-header miss.
- Managed buffers are different: `buffer_alloc` creates an old-arena `GC_TYPE_BUFFER` allocation (`buffer/header.rs:982`), and foreign-backed wrappers still have a managed `GC_TYPE_BUFFER` wrapper (`buffer/header.rs:1005`). Those do not need a Buffer registry question to establish storage kind.

## Callers from the profile

- `gc_pointer_and_type_from_value`: now reads one allocator-proven header first and asks Buffer/typed-array registries only when no tracked header exists (`native_call_method.rs:1160`).
- `class_vtable_fast_guard`: consumes the already-computed `NativeReceiverClass`; `Gc(GC_TYPE_OBJECT)` makes Buffer/typed-array impossible before the vtable/object loads (`native_call_method.rs:243`).
- `js_native_call_method`: classifies once at the call boundary and carries the answer into class and handle dispatch (`native_call_method.rs:1381`, `:2122`). Typed feedback now forwards its `site_id` instead of dropping it (`typed_feedback/guards.rs:853`).
- `object_static_prototype`: `meta_capable_object` now requires allocator ownership and `GC_TYPE_OBJECT`; headerless cells go to the residual prototype map without a Buffer probe (`object/prototype_chain.rs:140`, `:302`). Proven ordinary objects and validated RegExps have separate no-classification accessors (`:333`, `:347`).
- `js_object_get_field_by_name` and `get_field_by_name_object_tail`: managed typed-array kind comes from the header/payload (`typedarray_props.rs:55`, `field_get_set/get_field_by_name.rs:647`); the tail resolves one tracked type and only asks registries when it is absent. The Promise brand check also uses tracked ownership rather than speculative header bytes plus two exclusion probes.
- `js_object_get_field_ic_miss`: the Buffer/TypedArray diversion now switches on a tracked GC type, with registry fallback only for headerless storage (`object/field_get_set/ic_miss.rs:686`).
- `dispatch_primitive`: its remaining typed-array registry question is confined to the residual untagged/raw-pointer arm (`object/native_call_method/primitive_methods.rs:4`, `:914`). Tagged strings, numbers, booleans, bigint, null, and undefined are classified as `Primitive` before any byte-storage registry (`native_call_method.rs:138`).
- `js_array_get_f64`: it has its own array/collection receiver classifier (`array/indexing.rs:450`) and was not changed in this runtime-receiver patch. Its remaining typed-array and Buffer probes are not native-call receiver probes.
- `js_segments_view_next`, `_segment`, and `_regexp_test`: `cursor_ptr` now requires POINTER_TAG, current arena membership, `GC_TYPE_OBJECT`, and fixed class id `0xFFFF_000E` (`intl/segments_view.rs:88`). External/headerless storage cannot pass the arena check. `_regexp_test` still validates its varying RegExp argument once at the exported boundary (`:380`, `:391`); `regexp_test_str_bounded` no longer repeats that validation (`regex.rs:1472`). The RegExp is not checked at view-open time because the expression is a call argument and can change on each loop iteration.

## Mechanism changed

- `NativeReceiverClass` separates primitives, managed GC kinds, headerless Buffer, headerless TypedArray, and other pointers (`native_call_method.rs:55`).
- Primitive tags return immediately. Managed addresses are admitted only by `try_read_tracked_gc_header`; the GC type is authoritative, and `ObjectHeader.class_id` is read only for `GC_TYPE_OBJECT` (`native_call_method.rs:138`). A 64-slot thread-local site cache stores only `(site_id, gc_type, class_id)`, never a raw GC pointer (`:92`). Every hit revalidates current type and class id. Headerless answers are not cached, so external/native registry lifecycle cannot leave a stale positive entry.
- `dispatch_handle` consumes the classification and no longer calls either registry for managed receivers. The redundant Buffer retry at the end of Map/Set dispatch was removed (`object/native_call_method/collection_methods.rs:302`).
- General prototype and named-field paths use the same allocator-proven type decision. Registry fallback remains only after a tracked-header miss or for a finer Buffer/Uint8Array/ArrayBuffer/SAB brand that actually affects the requested operation.

These are code facts, not address-distribution assumptions: plain objects, boxed primitive wrappers, segment cursors, and RegExp cells are Perry allocator-owned allocations with explicit GC types; a segment cursor additionally has the fixed class id. External buffers and SAB backing blocks are allocator-untracked and therefore take the residual path.

## Diagnostics and sabotage tests

The before commit appends one whole `[native-call-diag]` line to the existing Buffer diagnostic, split into Buffer and typed-array counts for `native_receiver`, `view_cursor`, `object_static_prototype`, `field_by_name`, `field_tail`, `dispatch_primitive`, and `other` (`hot_diag.rs:1189`). It is deliberately a separate pre-fix SHA so perrymaster can obtain the caller split without mixing in the cut.

Named load-bearing tests added:

- `cached_plain_object_receiver_probes_zero_buffer_registries`: arms both registries, warms one site, asserts a second plain-object call moves neither registry counter, and asserts the site cache hit counter did move (`receiver_class_tests.rs:42`). The object has an own property literally named `buffer`.
- `cached_site_revalidates_when_plain_receiver_becomes_buffer`: substitutes a real `Buffer.from("A")` result at the same site and requires Buffer `toString` to return `A` with zero managed-receiver registry probes (`receiver_class_tests.rs:108`).
- `buffer_uint8array_and_arraybuffer_keep_their_method_paths`: checks the Uint8Array-branded Buffer representation and ArrayBuffer `slice` result brand (`receiver_class_tests.rs:138`).
- `external_buffer_and_sab_backed_view_keep_native_dispatch`: checks a headerless registered external Buffer and an Int32Array view over a process-global SAB (`receiver_class_tests.rs:161`).
- `cached_receiver_kind_does_not_hide_reassigned_prototype`: changes the receiver prototype after warming the site and requires the next call to invoke the new method (`receiver_class_tests.rs:215`).
- `primitive_receiver_tag_skips_byte_storage_registries`: arms both registries and requires a tagged-string call to move neither counter (`receiver_class_tests.rs:81`).
- `view_cursor_brand_is_a_class_load_with_zero_registry_probes`: requires zero registry-counter delta, corrupts only the cursor class id, requires immediate decline, restores it, and requires advance (`intl/segments_view.rs:799`).
- `view_regexp_pointer_is_validated_exactly_once_per_call`: requires the RegExp validation counter delta to be exactly one; restoring the duplicate makes it two and deleting the boundary check makes it zero (`intl/segments_view.rs:836`).

## Local gates

- `cargo build --release -p perry-runtime --features wasm-host -j4`: 2 compiler runs. First reached the changed code and failed with four mechanical pointer/unsafe compile errors. After fixes, the retry passed: `Finished release profile ... in 1m 57s`.
- `cargo test -p perry-runtime --release --lib -j4 -- --test-threads=1`: 1 compile attempt, 0 tests executed. It found that the test fixture could not reach the already-public `js_buffer_register_external` through the private `buffer::header` module. The function was then re-exported from `buffer/mod.rs`. The gate was not rerun because `df -g /` reported 11 GB free, below the binding 12 GB cutoff.
- `cargo build --release -p perry -j4`: not run; prohibited by the same 11 GB disk cutoff.
- Archive symbol check: Apple `/usr/bin/nm -g` could not parse Rust's LLVM 23 object attributes. The toolchain-matching `llvm-nm -g target/release/libperry_runtime.rlib` found `_js_buffer_register_external`, `_js_native_call_method`, `_js_object_get_field_by_name`, `_js_segments_view_next`, `_js_segments_view_segment`, `_js_segments_view_regexp_test`, and `_js_typed_feedback_native_call_method`.
- No local cc run was made. No real `HOME` was used.
- The final field-IC residual gate commit `72b713b99` was made after the successful archive build and was not cargo-compiled locally because the disk cutoff was already active. It is a header-directed replacement of the two registry conditions at `ic_miss.rs:686`.

## Exact perrymaster request and falsifiers

This is runtime-only: relink the I7-view tree against the branch runtime/archive using the existing bundle cache. Do not recompile the bundle for this patch.

1. Relink/run the before-counter SHA `8ad6e0777c0104910c6db66e5e4b16862c95f6b0` with `PERRY_BUFFER_DIAG=1`. For one 3300-character reply, retain the complete `[buffer-diag]` and `[native-call-diag]` lines, including every Buffer and typed-array caller bucket.
2. Relink/run code SHA `72b713b99abdaf54b07a06b8fe29c177d2a47ff5` identically. Prediction: `[buffer-diag] probes` falls from about 11.5 million to about **0.8 million** (hard falsifier: must be `<= 1,000,000`); `rejected` is predicted below **100,000** and should be near zero relative to the old 11.2 million. Remaining native-call diagnostic counts should be zero for managed `native_receiver`, `view_cursor`, `object_static_prototype`, and managed field paths; nonzero residuals must correspond to headerless external/SAB/native-view questions.
3. Perf the same I7-view main-thread reply at 999 Hz. The named buffer/typed registry group must move from 125/2373 self samples (5.27%) toward `<= 1%`; `js_native_call_method` inclusive samples must decrease. Report raw samples and denominator, not only percentages.
4. Run 5 paired 3300-character turns against `main` and this runtime, alternating order. Report each turn CPU, paired ratios, median ratio, and RSS. Expected RSS is unchanged; the allowed acceptance envelope is +1–10%, but any repeatable allocation growth is evidence against this mechanism because the cache stores 64 fixed, pointer-free entries per runtime thread.

Failure of the probe ceiling, near-zero rejection prediction, class/prototype substitution tests, view class sabotage, or exact-one RegExp validation delta falsifies the change.

# `lookup_typed_array_kind` miss audit (#9347)

Point-in-time record, measured on upstream `main` at `028b066bcf` (macOS arm64, Node 26.12.1).

## What changed since the issue was filed

#9347 was filed when a plain `Uint8Array` was a `BufferHeader` in the buffer registries, so
`lookup_typed_array_kind` could never answer for it. That premise no longer holds: since #10694 the
lookup reads the cell's own GC type byte and answers `Some(kind)` for every type in
`0x40..=0x4b` (`gc::is_typed_array_type`), and a plain `Uint8Array` is
`GC_TYPE_BUFFER_UINT8ARRAY = 0x40`. The 197 sites in the issue were re-enumerated on this commit
(204 production sites; 28 test-only sites skipped).

The cell that is a typed array in JavaScript but still misses is the Node `Buffer`
(`GC_TYPE_BUFFER = 0x4c`, and its view form `0x6c`). So each site was asked: *what does it do
with a `Buffer` receiver when the lookup misses?* `ArrayBuffer`, `SharedArrayBuffer`, `DataView`
and the key-material cells also miss, and for them a miss is the correct answer.

Classes:

- **FINE-ARM**: a buffer arm next to the lookup accepts `Buffer`.
- **FINE-GENERIC**: a miss falls to a generic path that handles `Buffer`.
- **FINE-NA**: `Buffer` cannot reach the site, or a miss is the right answer.
- **SUSPECT**: a miss is treated as "not a typed array" without an arm that accepts `Buffer`.
- **SUSPECT?**: like SUSPECT, but not confirmed reachable from a program.

The static classification below was produced by reading each enclosing function. Every SUSPECT and
SUSPECT? row was then compiled into a program and compared with Node (results in the next
section).

## Measured divergences from Node (`const b = Buffer.alloc(8)` unless noted)

| operation | node | perry `main` | site | resolution |
|---|---|---|---|---|
| `Atomics.load(b, 1)` / `add` / `store` | value | `TypeError` | `atomics.rs:208` (arm takes `is_uint8array_buffer`, which excludes `Buffer`) | fixed (brand PR) |
| `Object.freeze(b)` / `Object.seal(b)` | `TypeError` (views with elements) | succeed | `object_ops_frozen.rs:299,426` via `typedarray_props::typed_array_owner_kind` (excludes `Buffer`) | fixed (brand PR) |
| `Object.preventExtensions(b); Object.isExtensible(b)` | `false` | `true` | same owner-kind gate | fixed (brand PR) |
| `Object.defineProperty(b, 1, {value: 55})` | writes the byte | ignored | `typed_array_define.rs:56`, owner-kind gate | fixed (brand PR) |
| `Object.defineProperty(b, 9, {value: 1})` | `TypeError` | expando | `typed_array_define.rs:56` | fixed (brand PR) |
| `Reflect.set(b, 1, 66)` / `Reflect.defineProperty(b, 0, …)` | writes the byte | ignored | owner-kind gate | fixed (brand PR) |
| `Reflect.deleteProperty(b, 0)` | `false` | `true` | owner-kind gate | fixed (brand PR) |
| `assert.deepEqual(Buffer.from([1,2]), new Uint8Array([1,2]))` | passes | `AssertionError` | `typed_array_equality.rs:25` | fixed (brand PR) |
| `console.table(b)` | index/Values table | `<Buffer …>` | `table.rs:293` | fixed (brand PR) |
| `new DatabaseSync(Buffer.from(":memory:"))` | opens | `TypeError` | `sqlite/database_sync.rs:646` | open: accepting the `Buffer` path also needs `DatabaseSync` to reject an explicit `undefined` options argument (node: `ERR_INVALID_ARG_TYPE`), which `node-suite/sqlite/database/options-validation` passes today only because the `Buffer` path is rejected first |
| `typeof b.join` (also `map`, `filter`, `forEach`, `reduce`, `sort`, `findLast`, `with`, `toReversed`) | `"function"` | `"undefined"` | `get_field_by_name_tail.rs` buffer arm: no `[[Prototype]]` fallback (not a lookup site) | fixed (inherited-methods PR) |
| `Uint8Array.prototype.toString.call(b)` | `"0,3,…"` | `"[object Uint8Array]"` | `Array.prototype.toString` reads `b.join` through `value/dynamic_object.rs:526`, whose by-name arm takes `is_uint8array_buffer` (excludes `Buffer`); the `Buffer` arm after it answers only `length`/`byteLength`/`byteOffset`/`buffer` | fixed (inherited-methods PR) |
| `new Proxy(u8, {}).length` (any typed array, also `Map#size`, `ArrayBuffer#byteLength`, `DataView` getters) | `TypeError` | the target's value | `proxy/get.rs`: a trap-less `[[Get]]` ran builtin getters on the target, not the proxy (not a lookup site) | fixed (Proxy PR) |
| `b.x = 1; Object.prototype.propertyIsEnumerable.call(b, "x")` | `true` | `false` | `has_own.rs:755` | fixed (brand PR, via the owner-kind gate) |
| `const o = Object.create(b); o[100] = 1; Object.keys(o).length` | `0` | `1` | `dyn_index.rs:876` | fixed (brand PR, via the owner-kind gate) |
| `Uint8Array.prototype.zz = 1; "zz" in b` | `true` | `false` | `typedarray_props.rs:1087` | open |
| `(b as any as number[]).reverse()` / `.sort(cmp)` | sorts the bytes | `TypeError` | `array/header.rs:709` (Array-typed receiver path) | open |

Static SUSPECT/SUSPECT? rows that **matched Node** when compiled: `splice_slice.rs:428`
(`number[]`-typed `slice`), `indexing.rs:663/943/1340` (`number[]`-typed get/set), `iter_methods.rs:796/856`
(`Array.prototype.findLast(Index).call(b, …)`), `header.rs:709` for `fill`, `slice_ops.rs:279`
(`Uint8Array`-typed `subarray`), `polymorphic_index.rs:431` (symbol key), `iter_methods.rs:1662` (non-callable `map` callback). Not reachable from a program:
`arithmetic.rs:787`, `native_module_dispatch.rs:218`. Perry-only APIs not compared with Node:
`component/value.rs:103`, `typedarray/mod.rs:193/210` (`NativeMemory.copy` rejects a `Buffer`).

Divergences found by the same probes that are not lookup misses, and are left alone here:
`util.inspect` of a typed array with more than 6 elements is not grouped over lines;
`Buffer.from(int16Array)` / `Buffer.from(float64Array)` produce an empty buffer; `Object.assign({}, i16)`
and `{...f64}` copy no indices for non-`Uint8` kinds; `crypto.getRandomValues(new Float64Array(n))` does
not throw; `Reflect.get(u8, Symbol.toStringTag, {})` answers the kind name instead of `undefined`;
`Object.create(u8).length` does not throw.

## Static classification of every production call site

Line numbers are from the enumeration at `64257c3159`; no `lookup_typed_array_kind` call was added or removed between that commit and `028b066bcf`, so only line offsets can differ.

| site (file:line) | function | class | note (<=15 words) |
|---|---|---|---|
| crates/perry-runtime/src/array/alloc.rs:694 | js_array_from_arraylike_holey_value | FINE-ARM | !is_registered_buffer; falls to js_array_from_value |
| crates/perry-runtime/src/array/concat_reverse.rs:111 | js_array_concat source | FINE-ARM | is_registered_buffer arm calls buffer_to_array |
| crates/perry-runtime/src/array/fill_extend.rs:43 | js_array_fill_f64_const_extend | FINE-ARM | is_registered_buffer, so per-index slow path |
| crates/perry-runtime/src/array/fill_extend.rs:120 | js_array_fill_f64_const_len_extend | FINE-ARM | same as :43 |
| crates/perry-runtime/src/array/fill_extend.rs:155 | js_array_fill_f64_iota_extend | FINE-ARM | same as :43 |
| crates/perry-runtime/src/array/fill_extend.rs:221 | js_array_fill_f64_iota_len_extend | FINE-ARM | same as :43 |
| crates/perry-runtime/src/array/flat_clone.rs:617 | js_array_clone | FINE-ARM | earlier is_registered_buffer arm returns buffer_to_array |
| crates/perry-runtime/src/array/from_concat.rs:472 | append_concat_arg | FINE-ARM | is_registered_buffer OR'd in; appended as one element |
| crates/perry-runtime/src/array/from_concat.rs:751 | iter source classifier | FINE-ARM | is_registered_buffer gives Snapshot |
| crates/perry-runtime/src/array/from_concat.rs:1160 | dense concat source probe | FINE-ARM | is_registered_buffer rejects the bulk path |
| crates/perry-runtime/src/array/from_concat.rs:1179 | dense concat source probe | FINE-ARM | same, after clean |
| crates/perry-runtime/src/array/from_concat.rs:1332 | dense bulk-copy source check | FINE-ARM | is_registered_buffer gives None (slow path) |
| crates/perry-runtime/src/array/from_concat.rs:1450 | real_array_source | FINE-ARM | is_registered_buffer gives None |
| crates/perry-runtime/src/array/generic.rs:270 | classify_pointer | FINE-ARM | is_registered_buffer gives IndexedNative |
| crates/perry-runtime/src/array/header.rs:638 | clean_arr_ptr_resolve | FINE-ARM | buffer_family_type_owned covers Buffer |
| crates/perry-runtime/src/array/header.rs:709 | typed_array_receiver | SUSPECT? | fill/fill_range/reverse/sort callers have no Buffer fallback; silent no-op |
| crates/perry-runtime/src/array/header.rs:889 | normalize_array_receiver | FINE-ARM | is_registered_buffer keeps the raw receiver |
| crates/perry-runtime/src/array/indexing.rs:100 | array_iteration_is_exotic | FINE-ARM | is_registered_buffer beside it |
| crates/perry-runtime/src/array/indexing.rs:129 | array_iteration_is_exotic_cleaned | FINE-ARM | is_registered_buffer beside it |
| crates/perry-runtime/src/array/indexing.rs:663 | js_array_get_f64 | SUSPECT? | pre-clean TA arm misses; clean nulls Buffer; returns NaN |
| crates/perry-runtime/src/array/indexing.rs:721 | js_array_get_f64 (post-clean) | FINE-NA | unreachable; clean_arr_ptr already nulled Buffer |
| crates/perry-runtime/src/array/indexing.rs:943 | js_array_set_f64 | SUSPECT? | Buffer arm unreachable after clean; write silently dropped |
| crates/perry-runtime/src/array/indexing.rs:999 | array_strict_index_write_guard | FINE-ARM | null or is_registered_buffer, so guard is a no-op |
| crates/perry-runtime/src/array/indexing.rs:1340 | js_array_set_f64_extend_strict_impl | SUSPECT | goes to set_f64_extend; Buffer rejected; fresh empty array returned |
| crates/perry-runtime/src/array/iter_methods.rs:236 | js_array_for_each | FINE-ARM | buffer_receiver_dispatch (is_uint8_view_buffer) runs above |
| crates/perry-runtime/src/array/iter_methods.rs:327 | js_array_map | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/iter_methods.rs:551 | js_array_filter | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/iter_methods.rs:655 | js_array_find | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/iter_methods.rs:735 | js_array_findIndex | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/iter_methods.rs:796 | js_array_find_last | SUSPECT? | no buffer_receiver_dispatch; elements likely read as undefined |
| crates/perry-runtime/src/array/iter_methods.rs:856 | js_array_find_last_index | SUSPECT? | same as findLast |
| crates/perry-runtime/src/array/iter_methods.rs:914 | js_array_at | FINE-ARM | is_registered_buffer arm reads the byte |
| crates/perry-runtime/src/array/iter_methods.rs:979 | js_array_some | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/iter_methods.rs:1060 | js_array_some_captureless | FINE-ARM | is_registered_buffer sends Buffer to js_array_some |
| crates/perry-runtime/src/array/iter_methods.rs:1151 | js_array_every | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/iter_methods.rs:1364 | js_array_reduce | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/iter_methods.rs:1662 | js_validate_array_map_callback | SUSPECT? | message only: Array-style text, not the TypedArray text |
| crates/perry-runtime/src/array/iter_object.rs:266 | typed_array_iter_arr | FINE-ARM | is_registered_buffer minus AB/DV arm |
| crates/perry-runtime/src/array/iterator.rs:45 | js_typed_array_for_of_admits | FINE-GENERIC | false, so for-of uses iterator protocol, which handles Buffer |
| crates/perry-runtime/src/array/iterator.rs:1136 | spread-to-array funnel | FINE-ARM | is_byte_indexed_buffer arm runs earlier |
| crates/perry-runtime/src/array/join.rs:807 | js_array_join | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/reduce_right.rs:49 | js_array_reduce_right | FINE-ARM | buffer_receiver_dispatch above |
| crates/perry-runtime/src/array/search.rs:81 | as_typed_array | FINE-ARM | callers check the buffer helper first |
| crates/perry-runtime/src/array/species.rs:340 | species_result_set_length | FINE-ARM | is_registered_buffer && !non_indexed throws as for TA |
| crates/perry-runtime/src/array/splice_slice.rs:428 | js_array_slice | SUSPECT | no buffer arm; BufferHeader read as ArrayHeader |
| crates/perry-runtime/src/atomics.rs:208 | atomics_view_arg | SUSPECT | buffer arm is is_uint8array_buffer (excludes Buffer); throws |
| crates/perry-runtime/src/atomics.rs:232 | atomics_wait_notify_view_arg | FINE-NA | needs Int32/BigInt64; node also throws |
| crates/perry-runtime/src/bigint_coerce.rs:25 | to_bigint | FINE-GENERIC | ToPrimitive, then ToBigInt, as node does |
| crates/perry-runtime/src/buffer/access.rs:55 | pointer_addr_from_value | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/buffer/access.rs:95 | decode_buffer_set_source | FINE-ARM | is_registered_buffer arm first |
| crates/perry-runtime/src/buffer/access.rs:150 | collect_buffer_set_bytes | FINE-NA | TypedArray variant only; Buffer takes the Buffer variant |
| crates/perry-runtime/src/buffer/access.rs:192 | bulk_source_is_bytes | FINE-NA | TypedArray variant only |
| crates/perry-runtime/src/buffer/bytes.rs:468 | copy_typed_range_inner | FINE-GENERIC | callers convert first via buffer_receiver_as_uint8_typed_array |
| crates/perry-runtime/src/buffer/copy_bytes.rs:128 | js_buffer_copy_bytes_from | FINE-ARM | registered-buffer arm first, element size 1 |
| crates/perry-runtime/src/buffer/from.rs:633 | js_uint8array_new | FINE-ARM | is_registered_buffer arm above copies the bytes |
| crates/perry-runtime/src/buffer/query.rs:45 | js_value_buffer_or_typedarray_data | FINE-ARM | js_buffer_is_buffer arm first |
| crates/perry-runtime/src/buffer/transcode.rs:95 | source_bytes | FINE-ARM | is_registered_buffer minus AB/DV |
| crates/perry-runtime/src/buffer/validate.rs:150 | validate_byte_length_arg | FINE-ARM | js_buffer_is_buffer OR'd in |
| crates/perry-runtime/src/builtins/arithmetic.rs:787 | classify_value_typeof (raw bitcast) | SUSPECT? | no buffer arm; raw Buffer word would give "number"; likely unreachable |
| crates/perry-runtime/src/builtins/console.rs:85 | console.log raw-pointer probe | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/builtins/formatting.rs:642 | format_jsvalue | FINE-ARM | is_registered_buffer else-if gives Buffer formatter |
| crates/perry-runtime/src/builtins/formatting.rs:851 | format_jsvalue (raw bitcast) | FINE-ARM | is_registered_buffer, then format_buffer_value |
| crates/perry-runtime/src/builtins/formatting.rs:1476 | nested-field formatter | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/builtins/formatting/collections.rs:167 | raw_heap_pointer_display | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/builtins/formatting/typed_array_equality.rs:25 | typed_array_view (deep equal) | SUSPECT | fallback is_uint8array_buffer; Buffer vs Uint8Array loose-equal false |
| crates/perry-runtime/src/builtins/formatting/util_format.rs:155 | util_format_s_inspects | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/builtins/numbers.rs:610 | js_number_coerce | FINE-GENERIC | generic js_to_primitive path |
| crates/perry-runtime/src/builtins/table.rs:293 | js_console_table_with_properties | SUSPECT | next arm is_uint8array_buffer; Buffer falls to console.log, no table |
| crates/perry-runtime/src/byte_cell_support/mod.rs:142 | register_typed_array (debug_assert) | FINE-NA | only real TA kinds passed; debug only |
| crates/perry-runtime/src/byte_cell_support/mod.rs:147 | typed_array_has_shared_backing | FINE-NA | callers already resolved a TA kind |
| crates/perry-runtime/src/byte_cell_support/mod.rs:388 | typed_array_for_byte_helper | FINE-NA | no production callers of its users |
| crates/perry-runtime/src/byte_cell_support/mod.rs:461 | strict_typed_array_from_raw | FINE-NA | perry-only NativeMemory; callers gate on lookup or need Uint32 |
| crates/perry-runtime/src/child_process/sync_run.rs:95 | cp_is_input_byte_value | FINE-ARM | is_registered_buffer && !is_any_array_buffer |
| crates/perry-runtime/src/child_process/v8_serde.rs:255 | write_heap_value | FINE-ARM | earlier is_registered_buffer arm |
| crates/perry-runtime/src/component/value.rs:103 | component encode_at (WIT) | SUSPECT | only KIND_UINT8 is a byte list; Buffer rejected (perry-only) |
| crates/perry-runtime/src/dgram/net.rs:279 | message_bytes_inner | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/fs/mod.rs:450 | numeric_fd_value | FINE-ARM | js_buffer_is_buffer checked first |
| crates/perry-runtime/src/fs/stream.rs:377 | bytes_from_value | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/fs/stream.rs:405 | is_direct_write_data | FINE-ARM | js_buffer_is_buffer accepts Buffer first |
| crates/perry-runtime/src/fs/stream/write_file_input.rs:77 | write_file_chunk_bytes | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/json/replacer.rs:139 | apply_to_json | FINE-ARM | !is_registered_buffer in same guard |
| crates/perry-runtime/src/json/replacer.rs:282 | dispatch_pointer_with_replacer | FINE-ARM | is_registered_buffer arm first, then stringify_buffer |
| crates/perry-runtime/src/json/replacer.rs:904 | stringify_value_pretty | FINE-ARM | is_registered_buffer arm first |
| crates/perry-runtime/src/json/stringify.rs:624 | stringify_value | FINE-ARM | is_registered_buffer first |
| crates/perry-runtime/src/json/stringify.rs:915 | stringify_value_depth | FINE-ARM | is_registered_buffer first |
| crates/perry-runtime/src/json/stringify.rs:1292 | stringify array element | FINE-ARM | is_registered_buffer first |
| crates/perry-runtime/src/json/stringify_object.rs:56 | member_to_json | FINE-ARM | is_registered_buffer check just before |
| crates/perry-runtime/src/json/stringify_tojson_probe.rs:730 | plain_object_member | FINE-ARM | is_registered_buffer OR'd in, then None |
| crates/perry-runtime/src/node_api_host/buffers.rs:94 | classify_typed_array_owner | FINE-ARM | else-arm is_typed_array_buffer includes Buffer |
| crates/perry-runtime/src/node_stream_constructors/introspection.rs:125 | js_node_stream_is_array_buffer_view | FINE-ARM | registered_view accepts Buffer |
| crates/perry-runtime/src/node_stream_constructors/introspection.rs:141 | js_node_stream_is_uint8_array | FINE-ARM | registered_uint8 (not AB/DV) accepts Buffer |
| crates/perry-runtime/src/node_stream_readwrite.rs:1612 | typed_uint8array_byte_chunks | FINE-ARM | Buffer becomes a single chunk, as in node |
| crates/perry-runtime/src/node_submodules/zlib.rs:706 | js_zlib_validate_buffer_arg | FINE-ARM | is_registered_buffer accepts |
| crates/perry-runtime/src/object/descriptor_state.rs:282 | note_descriptor_target_edits | FINE-NA | acts only on object owners |
| crates/perry-runtime/src/object/descriptor_state.rs:538 | holder_word_blocks_store | FINE-NA | non-object header returned earlier |
| crates/perry-runtime/src/object/descriptor_state.rs:737 | plain_custom_prototype_may_intercept | FINE-NA | non-object proto returned true earlier |
| crates/perry-runtime/src/object/descriptor_state.rs:849 | plain_data_write_may_intercept | FINE-NA | caller guarantees GC_TYPE_OBJECT |
| crates/perry-runtime/src/object/field_get_set/entries_shape.rs:44 | js_object_entries_shape | FINE-ARM | registered_buffer_enum at entry |
| crates/perry-runtime/src/object/field_get_set/enumeration.rs:251 | js_object_keys_value | FINE-GENERIC | keys fallback reaches registered_buffer_enum |
| crates/perry-runtime/src/object/field_get_set/enumeration.rs:897 | js_object_values_value | FINE-GENERIC | values_shape reaches registered_buffer_enum |
| crates/perry-runtime/src/object/field_get_set/enumeration.rs:1013 | js_object_entries_value | FINE-GENERIC | entries_shape reaches registered_buffer_enum |
| crates/perry-runtime/src/object/field_get_set/enumeration.rs:1283 | js_object_keys_shape | FINE-ARM | registered_buffer_enum at entry |
| crates/perry-runtime/src/object/field_get_set/enumeration.rs:1734 | js_object_values | FINE-ARM | registered_buffer_enum first |
| crates/perry-runtime/src/object/field_get_set/get_field_by_name.rs:1007 | get_field_by_name (TA named read) | FINE-NA | enclosing gate excludes Buffer; buffer branch handles it |
| crates/perry-runtime/src/object/field_get_set/get_field_by_name.rs:1690 | promise named-read guard | FINE-ARM | !is_registered_buffer beside it |
| crates/perry-runtime/src/object/field_get_set/get_field_by_name_tail.rs:455 | get_field_by_name_object_tail | FINE-ARM | earlier registered-buffer arm handles Buffer props |
| crates/perry-runtime/src/object/field_get_set/has_property.rs:670 | js_object_has_property (`in`) | FINE-ARM | next is_registered_buffer arm |
| crates/perry-runtime/src/object/field_get_set/ic_miss.rs:302 | IC-miss get fallback | FINE-ARM | is_registered_buffer OR'd in, then generic get |
| crates/perry-runtime/src/object/field_set_by_name.rs:665 | js_object_set_field_by_name | SUSPECT | numeric name on Buffer skips expando arm, misses, tail drops write |
| crates/perry-runtime/src/object/global_this/bigint_promise.rs:321 | bigint_to_bigint_arg | FINE-GENERIC | miss goes to js_to_primitive (Buffer toString) |
| crates/perry-runtime/src/object/global_this/typed_array.rs:450 | typed_array_receiver (%TA% getters) | FINE-ARM | is_typed_array_buffer includes NodeBuffer |
| crates/perry-runtime/src/object/key_attrs.rs:1071 | attrs_live_in_keys_of_bits | FINE-NA | requires object gc type |
| crates/perry-runtime/src/object/map_set_subclass.rs:69 | instance_object_ptr | FINE-ARM | is_registered_buffer gives None |
| crates/perry-runtime/src/object/native_call_method.rs:851 | gc_pointer_and_type_from_value | FINE-ARM | is_registered_buffer first, so GC_TYPE_BUFFER |
| crates/perry-runtime/src/object/native_call_method/handle_methods.rs:244 | handle method dispatch | FINE-ARM | is_registered_buffer, then dispatch_buffer_method |
| crates/perry-runtime/src/object/native_call_method/object_proto.rs:370 | toLocaleString fold | FINE-ARM | is_byte_indexed_buffer, then dispatch_buffer_method |
| crates/perry-runtime/src/object/native_call_method/primitive_methods.rs:845 | dispatch_primitive raw TA arm | FINE-NA | untagged words only; Buffer dispatched earlier |
| crates/perry-runtime/src/object/native_call_method/typed_array.rs:302 | TA method tower (toLocaleString) | FINE-NA | reached only after a lookup hit |
| crates/perry-runtime/src/object/native_module_crypto_random.rs:74 | random_fill_sync | FINE-ARM | is_registered_buffer, elem_size 1 |
| crates/perry-runtime/src/object/native_module_dispatch.rs:218 | `$typed_kind` closure | SUSPECT? | is_uint8array_buffer arm; used only by non-node util.isUint8Array; probably unreachable |
| crates/perry-runtime/src/object/object_ops/define_property.rs:1032 | apply_property_descriptor | SUSPECT | buffer arm (~l.389) stores index keys as expandos |
| crates/perry-runtime/src/object/object_ops/has_own.rs:546 | has_own_of_non_ordinary_kind | FINE-ARM | buffer_like && is_registered_buffer arm first |
| crates/perry-runtime/src/object/object_ops/has_own.rs:755 | js_object_property_is_enumerable | SUSPECT | named key falls to ObjectHeader reads of a BufferHeader |
| crates/perry-runtime/src/object/object_ops/keys_array.rs:599 | define_append_transition_eligible | FINE-NA | non-object returned false earlier |
| crates/perry-runtime/src/object/object_ops/prototype.rs:551 | get_prototype_of_resolved | FINE-ARM | is_node_buffer closure gives Buffer.prototype |
| crates/perry-runtime/src/object/object_ops_frozen.rs:299 | js_object_freeze | SUSPECT | no throw; mark_all_keys walks a BufferHeader |
| crates/perry-runtime/src/object/object_ops_frozen.rs:426 | js_object_seal | SUSPECT | same as freeze |
| crates/perry-runtime/src/object/own_override.rs:297 | array_site_object_receiver | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/object/own_override.rs:733 | push_object_method | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/object/polymorphic_index.rs:204 | js_object_get_index_polymorphic | FINE-ARM | is_registered_buffer arm uses js_buffer_get |
| crates/perry-runtime/src/object/polymorphic_index.rs:271 | js_object_get_index_polymorphic | FINE-ARM | buffer arm already returned |
| crates/perry-runtime/src/object/polymorphic_index.rs:431 | js_object_set_index_polymorphic | SUSPECT? | symbol key on Buffer dropped |
| crates/perry-runtime/src/object/polymorphic_index.rs:443 | js_object_set_index_polymorphic | FINE-ARM | numeric key goes to js_buffer_set_value |
| crates/perry-runtime/src/object/polymorphic_index.rs:491 | js_object_set_index_polymorphic | FINE-ARM | buffer arm above returns first |
| crates/perry-runtime/src/object/property_ops/set.rs:187 | array_ptr_from_value | FINE-ARM | is_registered_buffer gives None |
| crates/perry-runtime/src/object/property_ops/set.rs:300 | set_integer_indexed_exotic | FINE-ARM | is_registered_buffer arm first, then js_buffer_set_value |
| crates/perry-runtime/src/object/reflect_support.rs:63 | obj_value_has_own_key | SUSPECT | Buffer arm says index not own; Reflect.set/defineProperty make expando |
| crates/perry-runtime/src/object/to_string_tag.rs:194 | typed_array_to_string_tag_name | FINE-ARM | buffer arm returns "Uint8Array", as node |
| crates/perry-runtime/src/object/to_string_tag.rs:345 | object_prototype_to_string | FINE-ARM | earlier buffer arm gives "[object Uint8Array]" |
| crates/perry-runtime/src/object/typed_array_define.rs:56 | typed_array_view_info | SUSPECT | arm is is_uint8array_buffer; miss means NotTypedArray, so ordinary define |
| crates/perry-runtime/src/object/typed_array_proto_thunks.rs:229 | ta_receiver_or_throw | FINE-ARM | is_typed_array_buffer includes NodeBuffer |
| crates/perry-runtime/src/object/view_brand.rs:90 | view_brand | FINE-ARM | buffer_brand runs first, giving NodeBuffer |
| crates/perry-runtime/src/os_process_stream_write.rs:104 | stdout write chunk bytes | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/promise/subclass.rs:61 | instance_object_ptr | FINE-ARM | is_registered_buffer gives None |
| crates/perry-runtime/src/symbol/get.rs:877 | @@iterator on TypedArray | FINE-ARM | is_byte_indexed_buffer arm just above |
| crates/perry-runtime/src/text.rs:470 | text_encoder_describe_received | FINE-ARM | falls to "an instance of Buffer" (node wording) |
| crates/perry-runtime/src/text.rs:528 | text_encoder_encode_into_dest | FINE-ARM | is_registered_buffer minus AB/DV |
| crates/perry-runtime/src/thread/clone_write.rs:547 | clone writer pointer dispatch | FINE-ARM | is_registered_buffer arm precedes |
| crates/perry-runtime/src/typed_feedback.rs:509 | raw_heap_type | FINE-ARM | is_registered_buffer first |
| crates/perry-runtime/src/typed_feedback.rs:603 | classify_array | FINE-ARM | registered-buffer arm before lookup |
| crates/perry-runtime/src/typed_feedback.rs:704 | helper_return_facts | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/typed_feedback.rs:1680 | masked_window_ta_kind | FINE-NA | only Int32/Uint32/Float64 count |
| crates/perry-runtime/src/typed_feedback.rs:2294 | array index-get fallback | FINE-ARM | registered-buffer byte arm right after |
| crates/perry-runtime/src/typed_feedback.rs:2681 | array index-set fallback | FINE-ARM | registered-buffer arm uses js_buffer_set_value |
| crates/perry-runtime/src/typedarray/access.rs:156 | js_typed_array_read_int32 | FINE-ARM | is_registered_buffer arm reads byte |
| crates/perry-runtime/src/typedarray/access.rs:222 | js_typed_array_read_f64 | FINE-ARM | is_registered_buffer arm reads byte |
| crates/perry-runtime/src/typedarray/access.rs:461 | TA.set source classification | FINE-ARM | next is_registered_buffer arm copies bytes |
| crates/perry-runtime/src/typedarray/access.rs:671 | js_uint8array_get | FINE-ARM | admitted_u8_read, then is_registered_buffer |
| crates/perry-runtime/src/typedarray/access.rs:726 | js_uint8array_index_get_value | FINE-ARM | admitted_u8_read, then is_registered_buffer |
| crates/perry-runtime/src/typedarray/access.rs:762 | js_uint8array_set | FINE-ARM | admitted_u8_write, then js_buffer_set |
| crates/perry-runtime/src/typedarray/access.rs:818 | js_ta_read_receiver_is_kind | FINE-GENERIC | proof fails, so unproven read path with buffer arms |
| crates/perry-runtime/src/typedarray/bigint.rs:82 | to_bigint_for_store | FINE-GENERIC | generic ToPrimitive path, as node |
| crates/perry-runtime/src/typedarray/construct.rs:63 | js_typed_array_new (raw-word arm) | FINE-NA | legacy untagged path; Buffer takes heap-source arm |
| crates/perry-runtime/src/typedarray/construct.rs:114 | typed_array_new_from_heap_source | FINE-ARM | next is_registered_buffer arm uses copy_from_uint8_buffer |
| crates/perry-runtime/src/typedarray/construct.rs:316 | typed_array_from_source_raw_values | FINE-ARM | is_registered_buffer leads to js_array_from_value |
| crates/perry-runtime/src/typedarray/construct.rs:445 | js_typed_array_new_from_array | FINE-NA | callers pass materialized arrays |
| crates/perry-runtime/src/typedarray/mod.rs:112 | js_typed_array_masked_window_data_ptr | FINE-NA | only after a kind proof; 0 means slow path |
| crates/perry-runtime/src/typedarray/mod.rs:193 | native_memory_copy_src_bytes | SUSPECT | is_uint8array_buffer arm excludes Buffer; throws (perry-only API) |
| crates/perry-runtime/src/typedarray/mod.rs:210 | native_memory_copy_dst_bytes | SUSPECT | same (perry-only API) |
| crates/perry-runtime/src/typedarray/slice_ops.rs:279 | js_typed_array_subarray | SUSPECT? | miss returns an empty Float64Array; reachable via Uint8Array-typed binding? |
| crates/perry-runtime/src/typedarray/transform.rs:45 | js_typed_array_to_reversed | FINE-NA | only caller converts Buffer to Uint8Array first |
| crates/perry-runtime/src/typedarray/transform.rs:263 | js_typed_array_to_sorted_default | FINE-NA | same |
| crates/perry-runtime/src/typedarray_props.rs:32 | typed_array_owner_kind | FINE-NA | gate excludes Buffer before lookup (that exclusion is itself a root cause) |
| crates/perry-runtime/src/typedarray_props.rs:1087 | typed_array_owner_kind_id | SUSPECT? | via has_property.rs:799; skips Uint8Array.prototype own-key check |
| crates/perry-runtime/src/validators.rs:248 | is_buffer_like | FINE-ARM | js_buffer_is_buffer first |
| crates/perry-runtime/src/validators/received.rs:253 | received_native_brand | FINE-ARM | buffer_brand NodeBuffer gives "Buffer" |
| crates/perry-runtime/src/validators/received.rs:348 | received rendering (typed flag) | FINE-ARM | `|| brand == "Buffer"` |
| crates/perry-runtime/src/validators/received.rs:399 | received rendering (length) | FINE-ARM | else-branch js_buffer_length |
| crates/perry-runtime/src/value/dyn_index.rs:239 | js_dyn_index_get (hot path) | FINE-GENERIC | miss goes to full dispatch with buffer arms |
| crates/perry-runtime/src/value/dyn_index.rs:330 | js_dyn_index_get (slow) | FINE-ARM | admitted_u8 and registered-buffer arms follow |
| crates/perry-runtime/src/value/dyn_index.rs:699 | js_dyn_index_set (hot path) | FINE-GENERIC | miss goes to slow setter chain |
| crates/perry-runtime/src/value/dyn_index.rs:803 | js_dyn_index_set (slow) | FINE-ARM | is_registered_buffer, then js_buffer_set_value |
| crates/perry-runtime/src/value/dyn_index.rs:876 | js_dyn_index_set (proto is TA) | SUSPECT | Buffer as [[Prototype]] missed; invalid index becomes own prop |
| crates/perry-runtime/src/value/dynamic_arith.rs:237 | dynamic add ToPrimitive | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-runtime/src/value/dynamic_object.rs:90 | js_value_length_f64 | FINE-ARM | is_registered_buffer arm first |
| crates/perry-runtime/src/value/dynamic_object.rs:187 | js_value_length_f64 (raw) | FINE-ARM | is_registered_buffer arm first |
| crates/perry-runtime/src/value/dynamic_object.rs:526 | js_dynamic_object_get_property | SUSPECT? | is_uint8array_buffer arm; other Buffer names read undefined; low reach |
| crates/perry-runtime/src/value/dynamic_object.rs:626 | Promise check | FINE-NA | Buffer returned at l.563 arm |
| crates/perry-runtime/src/value/to_string.rs:235 | js_jsvalue_to_string_impl | FINE-ARM | buffer arm above, js_buffer_to_string |
| crates/perry-runtime/src/value/to_string_primitive.rs:508 | ordinary_to_primitive_number_for_add | FINE-GENERIC | generic valueOf/toString via by-name get |
| crates/perry-runtime/src/webassembly.rs:134 | extract_bytes | FINE-ARM | else-if is_registered_buffer |
| crates/perry-stdlib/src/crypto/kdf.rs:361 | argon2_value_bytes | FINE-ARM | is_registered_buffer arm right after |
| crates/perry-stdlib/src/crypto/random.rs:782 | js_crypto_random_fill_sync | FINE-ARM | else-if is_registered_buffer |
| crates/perry-stdlib/src/crypto/util.rs:64 | bytes_from_ptr | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-stdlib/src/sqlite/backup.rs:203 | bytes_from_path_like | FINE-ARM | is_registered_buffer OR'd in |
| crates/perry-stdlib/src/sqlite/bind.rs:248 | is_named_parameter_object | FINE-ARM | !is_registered_buffer excludes Buffer |
| crates/perry-stdlib/src/sqlite/database_sync.rs:646 | node_sqlite_path_bytes | SUSPECT | is_uint8array_buffer arm; Buffer path treated as URL, throws |
| crates/perry-stdlib/src/sqlite/node_db.rs:225 | sqlite_image_bytes | FINE-ARM | registered non-AB/DV arm |
| crates/perry-stdlib/src/sqlite/session.rs:204 | changeset_bytes_from_value | FINE-ARM | registered non-AB/DV arm |
| crates/perry-stdlib/src/streams.rs:988 | is_byte_stream_enqueue_chunk | FINE-ARM | is_registered_buffer && !AB |
| crates/perry-stdlib/src/streams.rs:2019 | js_readable_stream_from_iterable | FINE-ARM | registered non-AB/DV arm |
| crates/perry-stdlib/src/streams/byob.rs:126 | view_info | FINE-ARM | buffer_brand NodeBuffer gives Brand::Buffer |
| crates/perry-stdlib/src/tls/module_api.rs:257 | ALPN protocols conversion | FINE-ARM | registered non-AB arm first |
| crates/perry-stdlib/src/webcrypto/util.rs:526 | webcrypto bytes coercion | FINE-ARM | is_registered_buffer OR'd in |

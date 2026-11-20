### perf(runtime): store array named properties with the array (brief 4 of #10166)

Named (non-index) properties of arrays — a regex `exec`/`match` result's `index`/`input`/`groups`/`indices`, user expandos, sparse far indices — no longer live in the address-keyed `ARRAY_NAMED_PROPS` side table, which charged every install a hash insert and `Vec`, every read a probe, every move a rekey, and every collection a `retain` over the whole table.

- **Exec results** store the values inline in slots in front of logical element 0, tagged with a fixed key set: installing them is 3–4 slot stores, and they no longer set `OBJ_FLAG_ARRAY_DESCRIPTORS`, so element reads on the result keep their fast paths (audited against every reader of that flag).
- **Other arrays** reserve one front slot pointing at a traced pairs array (insertion order); **full or >64-element arrays** keep an address-keyed fallback table instead of moving.
- The reserve uses the dense-queue front offset (`array/storage.rs`), so no element address changes and ordinary arrays pay nothing. The collector visits the reserve slots as fixed child slots (`gc/layout_slot_visit.rs`); `js_array_grow` carries them; `shift_dense`'s empty-queue reset keeps them out of `capacity`.

cgu=1 `instructions:u`: `re.exec` loop −9.2 %, exec-result creation −11 %, `m.index`/`m[1]` reads −26 %, `m[1]`/`m[2]`/`m.length` reads −71 %, non-global `match` −2.0 %; expando reads and overwrites slightly faster. Known cost: adding a new named key is ~3 % more (~+1k on a ~30k-instruction generic `[[Set]]`).

Also fixes two readers in `object/object_ops/prototype.rs` that tested `OBJ_FLAG_TYPED_ARRAY_PROTO` (bit 8, shared with `GC_ARRAY_NAMED_PROPS`) without checking `obj_type`, and `js_dyn_index_get`'s array fast path, which read physical slot `i` instead of logical element `i` after `shift`.

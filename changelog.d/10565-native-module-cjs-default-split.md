**`object/native_module.rs` split for the 2000-line cap.**

`native_module.rs` sat at exactly 2000 lines, so #10565's five-line addition
(`http`/`https`/`http2`/`net` in the one-object-per-module list) pushed it to 2005 and
tripped `scripts/check_file_size.sh`. CJS default-export resolution —
`cjs_default_base_module`, `cjs_default_namespace_name`, `create_cjs_default_namespace`,
`cjs_default_export_value` and `native_module_get_builtin_module_value` — moves to
`object/native_module/cjs_default.rs`, leaving 1942 lines.

The bodies are byte-identical; the one `super::` path inside them meant `object::`, which one
level deeper is spelled `crate::object::`. The parent re-exports the three names that are
reached from elsewhere, so every existing path still resolves: `native_module/constants.rs`
calls `cjs_default_export_value`, and `process.rs` reaches
`native_module_get_builtin_module_value` through `crate::object::`.

The region was chosen to carry **no raw-handle debt sites**. All four of the file's recorded
sites are in the vtable-access block, so `native_module.rs` keeps its ceiling of 4 in
`scripts/raw_handle_debt_files.txt` and the new module is not listed. That matters because
`raw_handle_debt.py --no-raise-vs` is strictly per-path monotone with no notion of a
relocation: splitting debt-carrying code out reads as "ceiling raised, was not listed at the
merge base" even when the total is unchanged. Recorded here because the next split of this
file will hit it — see #10583.

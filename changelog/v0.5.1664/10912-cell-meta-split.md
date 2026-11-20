Split the cell-metadata edge out of `object/mod.rs` into `object/cell_meta.rs`.

`object/mod.rs` reached 2020 lines — this train adds 41 across its ten PRs and
the file was at 1979 after v0.5.1632's `meta_flags` split. Moved
`cell_meta_slot` and its `#[cfg(test)]` companion `cell_has_meta_edge` (62 lines
with their doc block), leaving 1961 with ~39 lines of headroom rather than a
shave that puts the next commit back at the cap.

Boundary chosen for two reasons beyond cohesion. It stops before the `#[inline]`
/ `#[cfg_attr(...)]` attribute pair at what was line 1881, because a backward
walk over comment lines swallows those and leaves a dangling attribute
("expected item after attributes"). And it excludes `cell_expando_ensure`, which
holds `object/mod.rs`'s single recorded raw-handle debt site — moving that would
land it in a module with no ceiling and fail `raw_handle_debt` differently.

The re-export of `cell_has_meta_edge` is `#[cfg(test)]`-gated to match its
definition; ungated it is an unresolved import in a non-test build.

Verified after the split, not before: `check_file_size`, `raw_handle_debt`,
`addr_class_inventory`, `gc_store_site_inventory`, `shape_descriptor_census` and
`gc_runtime_root_holders` all rc=0, and `cargo check -p perry-runtime
--all-targets` under `-D warnings` is clean.

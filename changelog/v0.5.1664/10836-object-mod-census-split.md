`crates/perry-runtime/src/object/mod.rs` reached 2007 lines and failed
`scripts/check_file_size.sh`'s 2000-line cap.

Moved `object_tables_census` — the `PERRY_GC_CENSUS` reporter for the object
module's per-thread side tables — into the new sibling `object/census.rs`,
re-exported by name (`pub(crate) use census::object_tables_census;`) alongside
the existing `class_registry_census` re-export. It is a reporting leaf: nothing
inside the object module calls it, and its only caller is `gc/census.rs`'s
aggregator, so the boundary costs nothing in coupling. `object/mod.rs` lands at
1967.

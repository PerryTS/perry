Split the descriptor-owner lifecycle out of
`crates/perry-runtime/src/object/descriptor_state.rs`, which had grown to 2042
lines and failed `scripts/check_file_size.sh`'s 2000-line cap.

The new sibling `object/descriptor_state/owner_lifecycle.rs` holds the complete
set of functions that key off an owner ADDRESS changing or going away:
`prune_dead_descriptor_owner_entries` and its minor-scoped twin, the
`remove_descriptor_owner_entries` helper they share with
`clear_object_descriptors`, the growth/evacuation re-key
`transfer_descriptor_owner`, and the metadata-rewrite-phase address translation
`rewrite_descriptor_owner` that `gc_scan.rs` applies to every owner it visits.
The install and lookup side of the same tables stays in the parent module, so
the boundary is "what happens to an entry when its owner moves or dies" rather
than an arbitrary line cut. This follows the `gc_scan.rs` / `young.rs` idiom
already in that directory: a private `mod` plus explicit named re-exports
(`pub(crate) use owner_lifecycle::{…}`), and `gc_scan.rs` now imports
`rewrite_descriptor_owner` by name instead of picking it up from the parent
glob.

Behaviour-preserving: no function body changed, only its file and, for
`rewrite_descriptor_owner`, its visibility (private -> `pub(super)`, so the
sibling scanner can still reach it).

Also unblocks #10824, which shares this file at the same 2042 lines.

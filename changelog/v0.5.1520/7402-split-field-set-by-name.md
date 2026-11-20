Split `crates/perry-runtime/src/object/field_set_by_name.rs` into topical
sub-modules (issue #7402). The file sat at exactly 2000 of the 2000 lines
`scripts/check_file_size.sh` allows and is not allowlisted, so the next line
added anywhere in it turned the required `lint` context red — and a red
required context means every subsequent merge bypasses a gate rather than
being blocked by one. Trimming comments back to exactly 2000 (as was done
earlier) is a fuse, not a fix.

Pure mechanical relocation — no behaviour change. Every moved statement is
byte-identical to its `origin/main` text; the only edits are four
file-private `unsafe fn`s becoming `pub(super)` so the new sibling modules
can reach them, and one now-dead `#[allow(unused_assignments)]` dropped from
the entry point (the assignments it covered moved out with the macro).

    field_set_by_name.rs          568  entry point + pre-rooting head
    field_set_by_name/tail.rs     963  rooted ObjectHeader write walk
    field_set_by_name/fast_paths.rs  284  non-rooting fast paths
    field_set_by_name/write_helpers.rs  170  key/diag utils, closure +
                                             class-mirror + namespace writes
    field_set_by_name/attr_variants.rs   87  nonenum / nonconfigurable

The tail's split point is the `RuntimeHandleScope` creation, chosen so the
`refresh_roots_after_alloc!()` macro (#7341) and all 16 of its call sites —
plus both `GC_STORE_AUDIT(INIT)` markers and the #7154/#6759 publication
ordering they document — stay together in one file with their order
untouched. No rooted step crosses a module boundary.

`scripts/addr_class_allowlist.txt` and `scripts/addr_class_ratchet_baseline.txt`
are re-keyed for the new paths: both are path-prefix/per-file-count keyed, so
a file split redistributes existing sites across new paths and would
otherwise read as a ratchet regression. No site was added or removed.

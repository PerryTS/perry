### Gates: split `gc/layout.rs` back under the 2000-line cap

The #7510/#7511 merge batch (#7532, #7535, #7536) pushed
`crates/perry-runtime/src/gc/layout.rs` to 2030 lines, tripping
`scripts/check_file_size.sh` on `main`.

Moves the GC slot-descriptor visitors — `visit_gc_layout_slot_descriptors`,
`visit_gc_rewrite_slot_descriptors`, `visit_gc_rewrite_slots` and their
`fixed_slot` helper — verbatim to `gc/layout_slot_visit.rs`, re-exported from
`gc/mod.rs` so every existing path resolves unchanged. They are a coherent
group: all three walk an object's payload and hand the collector either a
read-only or a mutable view of each pointer slot. 1809 + 228 lines, both under
the cap. No behaviour change.

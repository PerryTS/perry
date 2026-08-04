**Fixed** the remaining six `mirror_class_object_static_write` call sites in
`js_object_set_field_by_name` passed a stale `obj`, completing #7381.

#7381 fixed two of the eight sites and scoped itself there on the theory that
the `refresh_roots_after_alloc!()` macro — which republishes `obj`, `key`,
`value` and `interned_key` together — could clobber an arm that rebinds `value`
locally. That theory was wrong: none of the eight arms rebinds any of the four
after its handle is taken, so republishing is a no-op except for the relocation
it repairs. Verified by measurement, not inspection — the full-coverage build
scores 58 pass / 2 fail on the object/assign/class/field/shape gap set, byte-identical
to pristine `main` (both failures pre-existing, one already in
`known_failures.json`).

With all eight refreshed, `test_gap_gc_assign_string_source_rooting`'s fault
leaves `mirror_class_object_static_write` entirely and surfaces the next catch in
the chain at `js_jsvalue_equals`, which remains open.

**Fixed a symbol name in codegen's root-reload allowlist that has never matched
anything (#5094).** `root_reload.rs`'s `NON_COLLECTING` listed
`js_gc_layout_note_slot`; the runtime exports `js_gc_note_slot_layout`
(`gc/layout.rs:814`) and `js_gc_note_slot_layout_aware` (`:833`). No symbol by
the old spelling exists anywhere in the tree — `gc_call_effects.rs` and all
twelve tests that reference these helpers use the real names.

The failure mode is why it survived. The file's own contract says a helper
missing from the list "is treated as collecting, which inserts a reload the
checker would not have demanded — a load, not a bug". So the typo was
safe-direction and silent: every emitted slot-layout note forced a root reload
instead of none, including the one call per guarded array element store
(`expr/index_set_guarded.rs`), which is the hot path #5094 exists for.

Both real names are now listed, and the phantom is removed from
`scripts/gc_root_dominance_check.py` as well — that file carried it too,
harmlessly, because it also carries the correct spelling. `_aware` is added
there alongside: it is `js_gc_note_slot_layout` behind an early return taken
when neither the new nor the old bits are pointer-bearing, so it does strictly
less than the entry point the set already admits — the same "differ only by
doing less" argument the file already accepts for `declare` vs `init`. The
one-way containment invariant (`root_reload.rs`'s list is a subset of the
checker's) is preserved in both directions of the edit.

Two regression tests in `root_reload_tests.rs`, both failing on the parent
commit: every `NON_COLLECTING` entry must be a name
`gc_call_effects::classify_direct_callee` answers `CannotCollect` for — which is
the "the two lists must agree" rule the checker's own comment states and nothing
enforced — and the two note helpers are pinned by name, because the bug was a
*missing* entry and a containment test alone is satisfied by an empty set.

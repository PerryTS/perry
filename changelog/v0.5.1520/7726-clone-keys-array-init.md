### Fixed

- **`js_object_clone_with_extra` published a half-built object whose `keys_array` slot held recycled heap bytes (#7683).** Both branches initialise `object_type`, `class_id`, `parent_class_id`, `field_count` and `meta` immediately after allocation, then set `keys_array` only at the end via `set_object_keys_array`. Between those two points sits `crate::array::js_array_alloc`.

  That call **allocates, so it can collect** — and the collector reads exactly this slot as a child edge (`object::gc_keys_array_slot`, enumerated by `gc_child_slots`). A collection landing in that window scans a pointer the mutator never wrote.

  The bytes are not zero. `arena_alloc_gc_old`'s fast path deliberately reuses a swept, **non-zeroed** hole (#7437: *"reuse a swept same-size hole … otherwise a block with any live object never yields its dead bytes back"*), so the slot holds real leftover heap content from whatever last occupied it. Whether that content happens to look like a plausible-but-unmapped address depends on allocation history — which is the shape of the ~1-in-102 `typed_feedback::object_shape` SIGSEGV reported in #7683.

  Every sibling allocator in `object/alloc.rs` already nulls the slot at this point. This function was the one that did not.

  **On the test, and why it checks source rather than behaviour.** The runtime version was written first: force a collection into the window (`force_next_general_arena_alloc_slow` + `GC_OLD_RECLAIM_PENDING`, the levers #7251 established), then assert the published clone's `keys_array` is sane. **It passed with the fix deleted.** Two independent reasons: by the time the function returns, `set_object_keys_array` has written the slot correctly, so nothing observable survives the window; and a fresh arena block is zeroed, so even inside the window the garbage reads as null unless the allocation lands in a recycled old-space hole with the right history.

  Reproducing it in-suite therefore needs a specific swept-hole layout **and** a collection landing in a few-instruction window. That is exactly why the fix is a by-construction initialisation rather than a guard, and why the guard asserts the invariant where it is decidable — the source, in the style of `scripts/gc_pin_sites.py`'s custody check for `GC_FLAG_PINNED`. Removing either initialisation fails the test.

  One note for anyone writing a similar source-level check: the first version matched the phrase `arena_alloc_gc` inside **its own explanatory comment**, registering a third allocation site and failing against correct code. It now strips comments before scanning. A source check that reads its own documentation as code is worse than no check.

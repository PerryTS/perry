### #6759 C3 rung 1 — the shape word is uniform (runtime)

`ObjectHeader.parent_class_id` **is** the shape word, but the stamp was gated on
`class_id == 0` — so a **class instance had no shape word at all**, and its only
header evidence of a key-set change was the `keys_array` POINTER. That is why
`class_field_inline_guard` compares that pointer, and why the #7916 header
shrink cannot delete it yet.

Rung 0 (#7981) removed the last reader of the header word as inheritance data,
which unblocked this. The `class_id` conjunct is gone; the rule is now uniformly
`the word is a ShapeId ⟺ is_shape_id(word)`.

★ **The emitted IR already spelled it that way.** All three PICs
(`property_get/generic_dispatch.rs`, `expr/proxy_reflect.rs` ×2) derive the
receiver token as `is_stamp ? (id | 1<<62) : keys_array`, where `is_stamp` is the
ShapeId *range* test and **no `class_id` is loaded at all**. This change makes
the runtime agree with the IR rather than introducing a new mode. The write-PIC
token derivations in `proxy/put_value.rs` never had the gate either.

`object/shapes.rs` grows `shape_word_is_writable` / `object_shape_stamp` /
`stamp_object_shape` / `clear_object_shape_stamp`, and the two clear sites
(`set_object_keys_array`, `delete_rest`), the two mint sites (`ic_miss`,
`get_field_by_name_tail`), the null→first-key birth stamp
(`field_set_by_name/tail`) and the `FIELD_CACHE` key route through them. A class
instance is stamped **lazily** at its first by-name resolve (codegen still
writes a constant `parent_cid` at `new C()`; eager birth stamping is rung 2),
and a `delete` on it now clears the stamp and re-mints a *distinct* id exactly
as a plain object's does.

**Free correctness rider:** two of the four mint sites had no RegExp-alias
check. A `RegExpHeader` aliases `GC_TYPE_OBJECT` with a different layout — its
offset 4 is the high half of `regex_ptr` (reads as `class_id == 0`, so the old
gate never excluded it) and **offset 8 is the low half of `pattern_ptr`**. All
four now refuse the alias.

★ **`typed_feedback::object_shape()` deliberately keeps its `class_id == 0`
gate**, and that is the finding rather than an omission. Its token is not a PIC
token: the guard family compares it against a **codegen-supplied keys pointer**
(`typed_feedback/guards.rs::method_direct_call_contract` requires
`shape_addr == expected_keys as usize`; the class-field and element-shape
contracts do the same). An id can never equal that pointer, so returning one
fails every such guard **closed** — memory-safe, no output difference, but it
silently deletes the direct-method-call route and the class-field fast paths.
Two existing tests catch it, and the site now names them. Migrating those nine
consumers is rung 3.

The entry gate
`delete_leaves_a_class_instance_with_no_shape_word_to_transition` went red as
designed and was replaced by the assertion its own failure message specified
(`delete_mints_a_fresh_shape_id_for_a_class_instance`), keeping the two halves
of it that are still true. New alongside it: same-class siblings share one
ShapeId and a delete moves only the deleted-from instance; a 3-level
`instanceof` chain still resolves after the header word is clobbered by a stamp;
and a stamped class instance primes the **id** token the emitted PIC computes
for it (priming a keys pointer there would be a permanent miss).
`pointer_token_prime_stamps_epoch_and_goes_stale_on_bump` lost its production
population — class instances were the last receivers priming raw keys pointers —
so it was split: the epoch mechanics now drive `pic_prime_get` directly, keeping
the `cache[2] == @PERRY_IC_EPOCH` guard provably able to fail.

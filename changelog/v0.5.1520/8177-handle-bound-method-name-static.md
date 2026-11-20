Fixed six runtime sites that bound a **movable GC heap string's interior** as a
bound-method name (#8133), on the timer-handle and TextDecoder/TextEncoder
property paths.

`js_class_method_bind(instance, name_ptr, name_len)` stores the name POINTER in
the bound closure and `dispatch_bound_method` re-reads it at CALL time, so the
pointer must outlive the closure — a contract codegen satisfies with per-module
rodata. Each of these callers instead derived
`key_ptr = (key as *const u8).add(size_of::<StringHeader>())`, the interior of a
heap string that is unreachable the moment the read returns, so a copying minor
could relocate or reclaim the bytes the closure names. #7747 fixed the identical
defect on the Buffer path; its commit message states the consequence — "whether
the stale bytes still spell the method is an allocator property, not a program
property" — which is why that one passed locally and took a SIGSEGV on
conformance-smoke.

Four sites are the ones #8133 names: the timer-handle arm in
`get_field_by_name_tail.rs` twice (NaN-boxed small-handle and already-stripped
handle-band receivers), and `text.rs`'s `text_handle_property` for
`TextDecoder.prototype.decode` and `TextEncoder.prototype.encode`/`encodeInto`.
Two more of the identical timer block were found while confirming those:
`ic_miss.rs`'s inline-cache MISS mirror (a separate LIVE entry point — its own
comment says the IC fast path funnels small handles there, bypassing the block in
`js_object_get_field_by_name`) and a third copy in `get_field_by_name.rs` that
looks shadowed by the tail today and is fixed defensively.

`is_timer_handle_method_key` is REPLACED by
`timer_handle_method_name_static(key) -> Option<&'static [u8]>`, and `text.rs`
grows `text_decoder_method_name_static` / `text_encoder_method_name_static`.
Answering the literal instead of a `bool` is the fix, not a refactor: with no
predicate left, a caller has nothing to pair with its own pointer, so writing the
obvious code cannot reintroduce the bug. `text_handle_property` goes one further
and no longer TAKES `key_ptr`/`key_len` at all — it cannot bind the caller's
pointer because it no longer has it.

**This one reproduces.** A literal `dec.decode` lowers the name to rodata and
never reaches these arms; a COMPUTED key does not. On a pre-fix binary,
`const k = "dec" + "ode"; const f = (dec as any)[k];` followed by 400k
allocations prints `decode=undefined` and then throws where node prints
`decode=hi` — a silent wrong answer with no instruments at all — and under
`PERRY_GC_ZEAL=1 PERRY_GC_ZEAL_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` it takes a SIGBUS the protector reports as
`RETIRED FROM-SPACE … retired_by_minor=#0 … obj_type=3`. Fixed, the same fixture
matches node byte-for-byte and exits 0 with the protector ARMED
(`retired_set=#0 blocks=18 bytes_protected=18874368`), so the green run means the
detector was live rather than that nothing was tried.

Six tests in `gc/tests/handle_bound_method_name.rs`, mirroring
`buffer_bound_method_name.rs`: they assert POINTER IDENTITY with the `'static`
literal, because — per #7747's note, which #8133 repeats — an inequality against
the key could pass with the bug present, and comparing the BYTES only fails on a
host where the freed memory has already been reused. Each also asserts its gate
is live (`is_known_timer_id` / `is_known_text_decoder_id`) before measuring, so a
green run cannot mean the arm never ran. Two sabotage arms were run and reverted:
the timer lookup echoing its argument (restoring the exact pre-fix pointer at all
four timer sites) fails the three timer tests plus the no-borrow test, and the
text lookups echoing theirs fails the two text tests.

One measurement of mine was vacuous before it was fixed: the tests initially
failed WITH the fix applied, because the helper compared against a `b"ref"`
literal written in the test file — two occurrences of the same byte string in
different modules are two `&'static [u8]`s the linker may leave at different
addresses, and it did. The expected pointer now comes from the lookup under test,
which is what `buffer_bound_method_name.rs` does and why it was right.

Two related surfaces are deliberately left for their own issue: the
primitive-receiver arm in `get_field_by_name.rs` (`(5).toString` &c.), and
`perry-stdlib`'s handle-property dispatch layer, where
`js_handle_property_dispatch` forwards the same pointer and eight sub-dispatchers
(`sqlite/dispatch.rs` ×4, `tls/dispatch.rs` ×2,
`common/dispatch/emitter_als.rs` ×2) capture it directly rather than remapping to
a literal — so `const f = db.run` / `emitter.on` / `als.getStore` carry the same
hazard.

`cargo test -p perry-runtime --lib`: 2424 passed, 0 failed, 4 ignored.

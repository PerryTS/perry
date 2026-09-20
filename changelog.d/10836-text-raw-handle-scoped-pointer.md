`alloc_text_object` (`crates/perry-runtime/src/text.rs`) rooted the freshly
allocated instance and then read the address back out of the handle three times
with a bare `get_raw_mut_ptr`, which `scripts/raw_handle_debt.py` counts as debt
in a module that has no ceiling.

All three are the scoped-argument shape, so all three become
`with_mut_ptr::<ObjectHeader, _>(…)` rather than a new ceiling: the prototype
link, the `object_meta_ensure` call that writes the packed decoder state, and
the final "hand the refreshed address back to the caller" read. The last one
follows the `obj.with_mut_ptr(|obj| obj)` idiom already used at the tail of
`node_stream_dispatch.rs`'s allocator. No ceiling added and no behaviour change
— each closure receives exactly the pointer the previous `get_raw_mut_ptr` call
would have returned.

Also unblocks #10831, which shares this file.

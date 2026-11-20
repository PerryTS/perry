**Added** `RuntimeHandle::across_{mut,const,nanbox}` and a raw-handle debt
ratchet, the layer-3 half of the rooting-by-construction argument in
`docs/src/internals/rfc-rooting-by-construction.md`.

A `RuntimeHandleScope` gives an object *liveness*: the collector marks it and
rewrites the slot. It does nothing for a raw pointer already read out of that
slot — that copy is invisible to the collector, and if the object moves it names
from-space. Every rooting bug fixed in the #7341 quarantine sweep (#7373, #7374,
#7375, #7376, #7381, #7383, #7385) had rooting **already**; what was missing in
each was ordering the re-read against the collection point.

`across_*` runs the allocating call and returns the object's post-collection
address in one step, so the pre-call pointer is never bound and cannot be reached
for by mistake. The `.size` arm of `js_object_get_field_by_name` is converted as
the worked example.

This is a debt counter, **not** a soundness proof, and the script says so. Rust
has no effect system to mark "this call may allocate", so no signature can reject
holding a stale copy across one; a `&mut Heap` token cannot be threaded through
`extern "C"` boundaries either. What the ratchet does is make the count of
unconverted sites visible and monotonically decreasing — 1006 across 110 files
today, wired into `test.yml` beside the address-classification audit.

Both the combinator tests and the ratchet were checked against their own negative
controls: sabotaging `across_mut` to return the pre-call pointer fails the test
with the intended message, and the ratchet fails on a rise and refuses to raise
its own baseline. The script's `--self-test` asserts the matcher still fires on
the shapes it exists to count and still ignores `across_*`, so a silently-broken
matcher cannot report zero and pass forever.

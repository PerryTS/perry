**The per-object layout death-prune walks its tables once instead of three
times, and no longer allocates a `Vec` of every live key** — 50.6 MB of a
compiled claude-code turn's 304 MB in the layout tables (#9792).

`prune_dead_per_object_layout_owners` visited every surviving key three times
per collection: `retain` to drop the dead owners, then
`layout_addr_filter_rebuild` — which first collected all of them into a
`Vec<usize>` — and then `recount_young_layout_records` to re-derive the
nursery-key count. The last two want exactly the survivor set `retain` is
already walking, so both fold into its closure. The `Vec` is gone from the
rebuild's other caller too.

The measurement that prompted it also found the accelerator these tables sit
behind unable to do its job. `layout_addr_filter_may_hold` is a 4,096-bit
one-hash sketch documented for "one or two entries, ~0.05 % false positives";
a new `PERRY_LAYOUT_DIAG` instrument reports **162,258 live keys and 4,096 of
4,096 bits set** on one 400-character claude-code reply. Every probe answers
"may hold", so the early returns in `transfer_per_object_descriptor` and
`transfer_per_object_slot_mask` never fire, and each rebuild was an O(live
keys) walk restoring the all-ones state it started from. Past four times the
bit count — 16,384 keys, where the false-positive rate is already 98.2 % — the
rebuild now sets all ones directly, the same conservative answer reached in
O(1); below that the filter keeps exactly the selectivity it has today. The
instrument says so out loud rather than leaving it to be inferred. Widening the sketch is not available
from the runtime: its geometry and hash are mirrored in `perry-codegen`'s
`emit_gated_forget_object_layout`, and discriminating at 162k keys would take
~190 KB of inline thread-local storage per thread.

`transfer_per_object_descriptor` also gained the emptiness test its shared
flag cannot express: the flag and the filter are common to both per-object
tables, so a full slot-mask table drags every relocation into the typed-layout
map as well — which on cc is permanently empty (typed=0, masks=162,258). One
`len` load replaces two hashes per evacuated object.

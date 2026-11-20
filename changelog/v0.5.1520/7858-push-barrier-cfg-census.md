**The push-barrier IR census walks the CFG instead of slicing text, closing #7708's last residue.**

`large_local_array_push_inbounds_store_emits_precise_slot_barrier` sat red for
two days because the write barrier moved into a dedicated `apush.barrier.*`
block downstream of `apush.realloc` — a legal block-structure change that a
text slice bounded by `apush.inbounds.`/`apush.realloc.` labels is structurally
unable to see. Three agents initially attributed the red to #7839; the failure
predates it (in #7708's list from 08-09), and the barrier was proven present in
the emitted IR all along.

The invariant is live — the fixture pushes a heap **pointer**, so the
store→layout-note→barrier ordering genuinely matters — so the census was
rewritten, not deleted: successors are walked breadth-first from
`apush.inbounds` within `apush.*` blocks, the store is required in the inbounds
block itself, and note/barrier must appear on the region in order.
Sabotage-verified: cutting the successor walk (which recreates the old
text-slice blindness) turns the test red.

Same remedy as #7698 applied to the class-field-store census; this makes the
full `cargo test -p perry-codegen` green modulo #7857 (filed separately,
window #7833..#7842).

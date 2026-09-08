**A relocated array whose element-shape proof does not exist no longer takes
the side table to find that out** (#9792).

`transfer_element_shape` runs from `layout_transfer` for every relocated
array — growth forwarding, a copying minor, an old-gen defrag. It already
computes `had_bit` for free from two header words it has read anyway, and
then took `ELEMENT_SHAPES`' `RefCell` and hashed both addresses regardless,
for two removes that on the overwhelmingly common path remove nothing. It now
returns when neither address advertises a proof.

The gate has exactly one safe shape and both halves are pinned by
`a_transfer_skips_the_table_only_when_neither_address_advertises_a_proof`.
Skipping on `!had_bit` alone would be wrong: a destination that still
advertises a proof describes storage the move has just replaced, so that case
keeps the full fail-closed path.

What skipping leaves behind is a record at an address whose bit is clear, and
that state was already part of the design rather than new: the bit is the sole
authority for a read (`element_shape_proof` returns `None` before touching the
table, and `note_element_store` is gated on the same bit), and `establish`
draws every identity from `ELEMENT_SHAPE_PROOF_SEQ` rather than from whatever
record sits at the address — precisely so a survivor cannot donate its epoch
to the next array proven there. `prune_dead_element_shape_owners` drops it on
the next collection, the same footprint-only guarantee a fail-closed transfer
already relied on. The test asserts both defences directly.

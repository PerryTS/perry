Serve a static-key store's learned way before any own-key lookup (#12484).
A key that lives in spill storage (an object literal born without room for a
later key-add, e.g. a literal passed as a parameter) is published to the
store site as a runtime way with the spill flip, which no emitted compare can
match. The miss entry asked the receiver's shape for an own writable binding
first and, while the site's ways had room, re-ran `prime_packed_set` after
every successful store, only to find the same way already published. Every
warm overwrite of such a key therefore paid the full key lookup and
publication walk (`prime_packed_set`, `object_shape_descriptor`,
`read_plan_lookup`, `try_own_store`): 873 instructions per iteration in the
§C1.4 `addkey` row's `param` x literal cells and 913 in the `varying` cells,
against about 60 for every other provenance.

The site's ways are a function of the receiver's ShapeId alone, so the miss
entry now consults them first, ahead of the shape's own-store answer, the
key-add memo and the warm transition. A warm overwrite of a spill-located key
is one ShapeId compare in the runtime and the existing checked spill store.
No new state: the ways and their serving code are unchanged, only their
position in the miss entry moved. `addkey` param x lit/factory: 873 -> 356
(num), 1001 -> 483 (str); varying x lit/factory: 913 -> 397 (num),
1042 -> 528 (str). The rest is the out-of-line spill store itself (the miss
call, the checked spill funnel and the spill read's leaf front).

Test: `a_learned_spill_way_is_served_before_the_own_key_lookup` plants a way
naming a different spill slot and shows the store lands there (it fails on the
old order), and `test_gap_12485_region_string_length_tree` covers add-then-store
loops on literal parameters, eight round-robin receivers and a receiver frozen
between loops.

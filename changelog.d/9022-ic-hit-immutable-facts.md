IC hits stop re-deriving shape-immutable facts, and the combined
overwrite+read loop goes past node: 31 → **24 ms against node's 29**. The
write-only loop drops to **15 ms against node's 23**.

Every stub or way hit re-proved the receiver's kind (a registry probe via
`is_class_object_ptr`), the plain-ordinary verdict, and the inline slot bound
(a descriptor fetch) — together 16% of the combined loop
(`write_fast_path_receiver_kind_ok` 6.8%, `shape_object_kind_by_id` 5.1%,
`shape_live_inline_slot_count_by_id` 4.4%).

All of those are facts of the SHAPE ID. The shape table only ever inserts
descriptors — its sole in-place mutations are GC bookkeeping (the relocated
`keys` address and the carrier liveness bits) — so `object_kind`,
`live_inline_slot_count` and `logical_key_count` cannot change under a fixed
id. A hit whose token matches the receiver's CURRENT stamp has therefore
already proved everything prime time proved about kind and bounds.

The slot word now carries the one prime-time verdict a hit needs:
`IC_SLOT_OVERFLOW_BIT`, choosing the inline region vs the spill store.
Overflow entries are bound-checked against `logical_key_count` at prime. Hits
keep exactly the checks that ARE mutable per object — header type, forwarded,
and the blocking flags the `Object.freeze` family sets — plus the token
compare. Applied to the write stub, the per-site dyn ways, and both read-stub
hit sites.

Interleaved A/B, min-of-21 at quiet load (~0.8): combined overwrite 31 →
24 ms; computed-key read, realistic-name read and populated delete unchanged;
write-only measured on the branch at 15 ms (node 23).

Verification: suite 2790 passed, all 60 lint gates pass, no warnings; the
adversarial, computed-key, and stale-slot differentials all byte-identical to
node.

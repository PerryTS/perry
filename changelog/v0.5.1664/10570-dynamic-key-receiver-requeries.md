Stop re-deriving the receiver on every dynamic-key property read. `o[k]` cost
674 instructions per access — in a loop, with a constant key, on a
two-property object — against node's ~4.5, and it did not amortize. A
symbol-resolved profile showed three per-access re-derivations, the same shape
as the array-push work: 674.1 -> 546.0, **-19.0%**.

`keys_find_slot_by_bytes` ran `clean_arr_ptr` on `descriptor.keys` (16.2% of
the loop) although that field is maintained by the COLLECTOR —
`shapes::scan_shape_table_rekey_mut` writes the forwarded address back into
every descriptor record when the keys array moves, and prunes descriptors whose
keys array died. `try_data_get_bytes` read a thread-local on every access to
ask whether the receiver is `process.env`; a sticky latch means that is touched
only once such an object exists. And `is_anon_shape_class_id` took an `RwLock`
read guard per access (16.1%, the largest frame left) to consult a set written
only from module init; it is now answered from a lock-free open-addressed
mirror, sound because the set is insert-only, and living in the class IMAGE
beside `parent_dense` because `ImageTable` resolves every access through
`current()`.

A sticky "is the anon-shape set empty?" latch was tried first and measured
ZERO — object literals ARE anon shapes, so the flag is true in essentially
every real program. Caching the resolved slot against the key was dropped: a
cached key pointer can be recycled into a WRONG slot rather than a miss, and
would need its own GC root scanner.

New fixture `test_gap_dynamic_key_read_paths.ts` covers the receiver shapes and
the mutations that must invalidate what the fast path reads — key added and
deleted after a first read, 64 keys to cross the indexed-lookup threshold,
declared-class instances, prototype-chain reads, accessors, and the
`.constructor`/`getPrototypeOf` verdicts the mirror answers. Byte-identical to
node; two GC-stress seeds with from-space protection and
PERRY_GC_FROMSPACE_SCAN_ABORT=1 ran 2,117 and 2,450 copying minors with
dangling=0 and missing_rewrites=0.

- **Set key identities: probes no longer insert, and minors skip old keys (#11169 follow-up).** When `has` or `delete` probed an indexed Set (more than 8 elements) with an object that was not a member, a permanent entry was added to `SET_KEY_IDENTITIES`. It stayed there until the probed object died. Lookups now use a read-only `existing_hash`, so a movable key without an identity is a definite miss. Only `insert_value` and `rebuild_index` allocate identities.

  The identity table is now split by the key's generation into young and old halves. A minor-scoped rekey scan and the new `young_prune` walk only the young half, and promoted keys migrate into the old half. Before, every minor walked the whole table.

  I tried a `YoungLog` first. It cost +16% wall and +34% RSS on the all-young `gc_ratchet` 08 Map/Set probe, because the log was as large as the table and was sorted on every pass. The split is neutral on that probe (0.24 s, 35.6 MB on both main and the branch).

  On a 10k-member Set probed 3M times with non-member objects: 195 → 57 ms, 56.2 → 39.5 MB max RSS.

  Tests in `gc/tests/copying/set_index.rs` cover miss-probes (identity count unchanged), members found after a copying minor, old keys visited 0 times by a minor's scan and prune, and the young prune. Each fix was sabotage-checked: reverting it turns the matching test red.

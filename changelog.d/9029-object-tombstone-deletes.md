O(1) object deletes via tombstones, flag-gated (`PERRY_OBJECT_TOMBSTONES=1`,
default OFF). With the flag on, `bench_populated_delete` drops
2089 → **1050 ms (−50%)**, with the combined overwrite and realistic-name-read
loops unchanged.

`delete obj[k]` on an OWNED keys array (no `GC_FLAG_SHAPE_SHARED` — the same
authority two existing call sites already trust for the clone-or-mutate
decision) writes a hole marker over the key slot and clears the value through
the barriered stores, exactly #9020's Map-tombstone idiom. Survivors keep
their slots: no value shift, no layout rebuild, no index shift, no live-bound
update. Tombstones are squeezed out when they reach half the slots, one
overlap-safe pass mirroring `compact_map_entries`.

**Shape identity per delete is forced, and its lifecycle is the hard part.**
The per-site dyn-IC ways live in generated-code globals the runtime cannot
reach, so retiring a deleted key's cached `(token, key) → slot` entries
requires changing the token itself: every hole-delete publishes a successor id
(fresh semantic generation + `hole_count`, now part of `ShapeFacts` identity
and covered by the facts-exhaustiveness test). The first version left every
predecessor id alive against ONE stable array address — the reverse-index list
grew per delete and every publish walked it, measuring **26× slower** than the
compacting delete. Retiring just the direct predecessor halved that;
delete-then-re-add cycles also mint an id on the APPEND side, so the publish
now sweeps EVERY stale id for the owned address (each is unreachable the
moment the header word is restamped; single owner by the same flag the gate
trusts). With the sweep the flag-on path is 2× faster than flag-off.

Walkers: `js_object_keys`' raw-push fast path, `getOwnPropertyNames`' walk,
and `JSON.stringify`'s field walk skip the marker explicitly. Note
`js_array_get` translates `TAG_HOLE` to `undefined` per OrdinaryGet (#323), so
key walks reading through it must skip BOTH forms — comparing `TAG_HOLE` alone
was dead code and let holes reach output as JSON `null`; `undefined` is never
a legal key, so the two-form skip is safe. Every other enumeration path
resolves keys through `js_string_key_bytes`, which rejects the marker.

Verification: suite 2799 passed, all 60 lint gates pass. Four differentials
byte-identical to node in BOTH flag states: the enumeration suite
(keys/values/entries/for-in/stringify/spread/rest across interleaved deletes,
re-adds, threshold crossings, and overflow-slot objects — this suite caught
the getOwnPropertyNames and hole-canonicalization bugs), the adversarial
property suite, the computed-key suite, and the stale-slot suite.

Remaining flag-on cost is the per-delete publish/sweep machinery (~5 µs/op vs
node's 0.1); reducing that is the follow-up before default-on.

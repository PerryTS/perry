Computed property reads take the key by value, and the combined
overwrite+read loop drops 38 → 30 ms (node: 26); the computed-key read loop
drops 21 → **13 ms against node's 24** — nearly twice as fast as node.

The computed-read lowering called `js_get_string_pointer_unified` before the
by-name entry, because that entry's signature wants a `*const StringHeader`.
For an SSO key that means materialising inline bytes onto the heap — an intern
hash and table probe on every read — purely to satisfy a pointer signature.
`intern_dispatch_bytes` was 5.5% of the combined loop, essentially all of it
that.

The new by-value entry (`js_typed_feedback_object_get_field_by_value_f64`)
takes the key NaN-boxed:

- a heap-tagged key is unmasked and passed through — no work;
- an SSO key probes the megamorphic read stub on its CONTENT bits — a hit
  never builds a `StringHeader` at all;
- a stub-missing SSO key takes a READ-ONLY intern probe (an intern hit cannot
  allocate or move anything) and calls through with the canonical pointer —
  still no rooting;
- only a key read before its first write materialises, with the receiver
  rooted across the allocation (the hazard the old codegen comment worked
  around by re-deriving its handle below the unbox — that workaround is gone
  because the fast path no longer allocates).

Everything downstream is unchanged: feedback recording, exotic receivers and
prototype resolution take exactly the previous path.

Interleaved A/B, min-of-21 at quiet load (~1.1): combined overwrite 38 →
30 ms, computed-key read 21 → 13 ms, realistic-name read and populated delete
unchanged.

Verification: suite 2790 passed, no warnings; adversarial property
differential, computed-key differential, and a targeted stale-slot
differential (192 reads across every rotation state of a delete/re-add cycle
with a stable keys-array address) all byte-identical to node.

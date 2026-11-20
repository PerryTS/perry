The post-delete value shift stops re-resolving the receiver's slot bound once
per moved slot.

`js_object_get_field` resolves the live inline-slot bound itself, and that
bound is a shape-table probe — so shifting N values down after a delete paid N
shape-descriptor lookups. A 500-key object did ~499 of them per `delete`. The
loop already holds that exact bound in `field_count`, and nothing in it changes
the receiver's shape.

`object_field_at_with_live` exists for precisely this case (#8122) and is
otherwise the identical body, inline-vs-overflow split included, so the values
read are unchanged.

This is also why `shape_descriptor_by_id` and `shape_slot_lookup` ranked so
high in the delete profile: a large share was this loop, not the index lookups
they exist to serve.

Interleaved A/B, min-of-15 at quiet load (~1.0): `bench_populated_delete`
4651 → **4371 ms, −6.0%** (mean −6.1%). Combined overwrite and realistic-name
read unchanged.

Cumulative with #9000/#9001/#9002: 5938 → 4371 ms, **−26.4%**. Suite 2787
passed, no warnings; adversarial property differential byte-identical to node.

Not done here: the shift still moves values one at a time through barriered
stores. The deferred-layout batching helper (#7630) would remove the per-slot
layout note, but its settle step marks the object layout-UNKNOWN whenever a
pointer was stored — trading a precise pointer mask for scan-all-slots on an
object that already had one. That is a GC-cost regression on an existing
object, so it was deliberately not used.

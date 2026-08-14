**First authoritative-shape prerequisite slice (#8067; #8047 remains blocked).**

`ShapeId` now resolves through the current agent's `RuntimeState` to a
descriptor containing a moving weak mirror of the rooted ordered keys array,
its logical key count, and the exact live inline-slot bound. A live object's GC
scan roots keys through the still-authoritative header edge and then synchronizes
the descriptor named by its ShapeId; the descriptor table itself remains weak
and prunes dead historical shapes, so it cannot make key arrays immortal. The pointer-keyed
key-to-slot table remains a validated accelerator, while a separate by-id map
holds immutable descriptor facts. Process-global, never-reused ids plus an
agent-local lookup mean a worker/realm-local table cannot resolve a foreign id
to an unrelated layout. Range exhaustion parks at the end without wrapping;
normal callers recover by leaving the object unstamped and using the retained
header pointer/count guards.

Keys transitions and live-slot changes now publish through central helpers.
They clear the old stamp, update the still-authoritative header fact, install a
complete exact descriptor, and only then publish its id; live-slot growth does
all of that before storing a pointer-bearing field value. Shared ordered-key
arrays still clone before append, so one sibling's transition cannot mutate the
artifact shared by another sibling's immutable descriptor. Delete and
defineProperty growth eagerly install the post-transition descriptor. The GC
object scanner visits the authoritative header edge, then immediately mirrors
an observed rewrite without exposing a movable HashMap bucket as a GC slot; a
metadata-only scanner repairs deferred forwarding after evacuation, and
post-trace pruning removes dead descriptors and indexes.

This slice deliberately does **not** migrate all generated PICs, replace the
class-object/RegExp discriminants, remove any `ObjectHeader` fields, or change
the runtime/FFI ABI. `ObjectHeader.keys_array` and `.field_count` remain the
source of truth and every previous guard remains as redundancy; debug parity
assertions check any published descriptor against them. A checked-in source
census records the exact normalized multiset of remaining legacy member/layout
consumers and rejects any unreviewed change. No performance or reduced-size
claim is made here.

Add object dictionary mode: a receiver can carry its own ordered key list
instead of interning a layout (#10868 step 2.5, stage 1). Default off.

**Why it lands before the content key.** Step 2.5 makes shape identity
canonical, so equal layouts intern to one shared record. Today 97.8% of shape
records are retired because a private shape dies with its object; a shared
record cannot be retired by ownership, so a workload producing unboundedly many
distinct key lists — a `Map`-like object built by name, a per-request object
keyed by user input — would accumulate interned shapes for the life of the
process. There is a cost half too: under one canonical keys array per layout an
append can no longer mutate in place, so an object whose key list is unique to
it pays a copy of length *k* per append, i.e. O(k²) over *k* appends.
Dictionary mode bounds both, and it is the only part of that work that touches
none of the content key's files.

**The representation.** A dictionary-mode receiver's ShapeId describes no keys
at all (`keys = NULL`, `logical_key_count = 0`, the live inline bound frozen at
the latch, a `semantic_generation` from a third namespace), and its real
ordered key list is a private `GC_TYPE_ARRAY` in a new
`ObjectMeta::dictionary_keys`. **Values do not move**: the key at position *i*
still reads inline slot *i* below the live bound and the object-owned spill
buffer at or above it. The mode relocates names, never values, which is what
lets the existing read, write, delete and enumeration code run on a dictionary
object unmodified.

A shape that claimed a key list the object no longer matched would be a silent
wrong value in every consumer that trusted it; a shape that claims *nothing* is
merely incomplete, so an unbranched consumer produces a missing property, which
a differential test against node catches on its first row. That is why the
shape goes keyless rather than stale.

**The branch is one function.** `object_keys_array` is the sole runtime
derivation of a receiver's ordered key list, so branching it there gives every
enumeration walk, `in`/`hasOwn`, `delete`, `JSON.stringify`, spread and
`Object.assign` node-identical behaviour with no second implementation of key
order, hole skipping or integer-key ordering. It costs nothing on an ordinary
receiver: a nonzero `keys` word returns before the branch.

Six fast paths did have to be taught, and five of them are the same defect:
`keys.is_null()` was read as "this receiver has no own properties". On a
dictionary object that is false, and two of the five (`ic_miss`'s inherited-read
primer and the own-field shadowing scan in `native_call_method`) would have
produced a **wrong value**, not a slow one — an own property answered from the
prototype chain, and a vtable method winning over an own field. Each now
declines, which is always correct because the generic path reaches the same
list through `object_keys_array`.

**Identity.** One ShapeId per dictionary object, drawn once at the latch —
O(1) per object against today's O(k). Appends mint nothing: the array's address
is not a fact of a shape whose `keys` word is NULL, and an append moves no
value, so a cache primed on the receiver stays correct. A republication that is
not an append (a compacting delete, which shifts values) does draw a fresh
generation, which is what invalidates those caches. Two dictionary objects must
never share an id — a compiled IC compares ShapeIds and nothing else — so the
draw comes from a third generation namespace, disjoint by construction from the
`SHAPE_SEMANTIC_NEXT` counter (bit 63 clear, aborts far below 2^62) and from
`deterministic_semantic_generation` (bit 63 set): dictionary draws set bit 62
and clear bit 63. `dictionary_generation_namespaces_are_disjoint` asserts it.

**GC.** `dictionary_keys` is a traced, rewritten child edge exactly like
`spill` (#6812): one `visit` in the `GcRewriteDescriptorKind::ObjectMeta` arm of
`visit_gc_rewrite_slot_descriptors`, which is the single enumerator the
non-copying minor mark, the full mark, the copying-nursery evacuation, the
whole-heap rewrite and the dirty-slot rescan all drive — mark, move and
remembered-set coverage from one line. Nothing in the tree enumerates
`ObjectMeta`'s fields (no derive, no registry; `validate_gc_type_info` pairs the
type KINDS, never the slot lists), which is how `expando` came to be missing
from the second, production-unreachable enumerator in `gc/layout.rs` — now
commented rather than left to be discovered again.
`test_object_meta_dictionary_keys_survive_copied_minor_move` is the sabotage
target: remove the `visit` and it reddens.

**The latch is stubbed off** and can only fire when explicitly armed. The
production trigger belongs to the content key: the condition that matters is
"this object's key list is unique to it", which is not answerable until
identity is content-keyed. Off, the predicate is one relaxed load and a
compare. Armed by `PERRY_OBJECT_DICTIONARY_MIN_KEYS=<n>` (value-parsed, not
presence-parsed — #7991 shipped a knob that `=0` turned on) or by
`test_arm_latch`, and `[object-dictionary] armed=… candidates=… latches=…
publications=… regenerations=…` prints under `PERRY_GC_DIAG` with zeros
included, so `armed=false` ("off"), `armed=true candidates=0` ("armed and never
reached" — the bug shape) and `armed=true candidates>0 latches=0` ("reached and
declined") are three distinguishable states rather than one silent zero.

`ObjectMeta` moved to `object/meta_record.rs` with its `offset_of!` pins: the
sixteenth word took `object/mod.rs` past the 2,000-line gate, and the record
and the transition cache were the two regions in that file owned by different
lanes.

A receiver already carrying tombstones is refused, because `hole_count` is a
fact a keyless shape does not carry and latching over one would drop it.
`a_receiver_with_holes_is_refused` states that as a decision rather than an
accident.

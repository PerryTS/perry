Gave every exotic cell type a metadata edge, and moved `Error`'s own properties
onto it — deleting a side table and all four of its GC hooks.

Cell types declare their fields independently; there is no shared header prefix.
So *"does this cell own an `ObjectMeta`?"* had no single answer, and only an
`ObjectHeader` could be asked. That is why per-object state for the exotic types
accumulated in tables keyed by the owner's **address** — there was nowhere on
the cell to put it. Errors alone carried seven such tables plus four GC hooks.

Every exotic cell now has a `meta` edge, reachable through one accessor
(`cell_meta_slot`): Object, Error, Map, Set, RegExp, Promise and Date. It
answers `None` for anything unmapped, so callers degrade to their existing
storage rather than mis-reading another layout as a pointer.

Each edge is **traced**, not merely rewritten. Where a type's rewrite arm is
also its mark path the slot goes there; RegExp delegates to the layout visitor,
so its edge goes in `gc_child_slots` instead. #6812 is exactly the bug of
choosing wrong — an edge visited only on the rewrite path is invisible to
marking, and the record is swept out from under a live owner.

`Date` needed more than a field: it was `pointer_free` with a `Leaf` (no-op)
descriptor, holding one raw `f64`. A cell with a pointer must be scanned, so it
moved to a new `MetaOnly` descriptor with `pointer_free = false`.
`validate_gc_type_info` caught the flag when an edit missed it.

The arena reuses free-list memory **without zeroing**, so an uninitialised meta
edge would be a garbage pointer the collector follows. Every allocation path
initialises it explicitly; `Promise` routes through `Promise::new`, so the
constructor covers its several sites.

`ObjectMeta` gains `expando`, a named-property bag for cells with no inline slot
layout. It is appended last because the struct's offsets are a contract with
codegen (`offset_of!` asserts at 32/48/56 — inserting mid-struct failed them).
`ERROR_USER_PROPS` is deleted along with all four of its GC hooks:
rekey-on-evacuation, finalize, dead-sweep and the root scanner. Error properties
are now an ordinary traced child edge that moves with its owner, dies with its
owner, and cannot be inherited by a later tenant of a recycled address.

The tracing tests assert slot **enumeration** directly rather than survival
across a collection. A survival test is vacuous here: arena block reset is
all-or-nothing, so `gc::trace` force-marks every object in a block that still
holds one reachable object (#7975), which keeps an untraced record alive anyway.
Verified by sabotage — deleting the visit line left the survival version passing
and fails the enumeration version.

No user-visible change on its own. This is the gate that lets the shape and
descriptor payloads move off address-keyed tables.

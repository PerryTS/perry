### `fix(gc/shape)`: root and rewrite the keys edge from the ShapeId descriptor, not `ObjectHeader.keys_array` (#8112)

`ObjectHeader.keys_array` was the last blocker for #8047's header shrink.
#8086 made the `ShapeId` descriptor authoritative for the keys *value*, but the
header word was still the only thing that **rooted** the keys array and the
only **rewritable location** the evacuator could hand to a slot visitor —
so deleting it would have unrooted every keys array in the heap. This lands the
missing GC protocol. `ObjectHeader`'s size is unchanged; #8047 shrinks it.

**The edge now lives in the descriptor.** `ShapeDescriptor` records are BOXED,
so each record's address is fixed for its lifetime, and a lifted descriptor
carries `record` — the address it came from — alongside the `keys` snapshot.
`object::gc_shape_keys_edge_slot` hands the collector `&mut record.keys`, an
ordinary child slot it marks through and rewrites in place. The address rides
on the descriptor `gc_child_slots` already resolved for the receiver, so the
edge costs no extra shape-table probe (#8122's one-probe rule).

`synchronize_live_object_shape_descriptor_after_header_visit` is **deleted**,
along with the fact-capture block in `visit_gc_layout_slot_descriptors` that
fed it. That callback existed only because the header was the strong edge and
the descriptor a weak copy to be repaired from it, under exact-facts validation,
once per traced receiver whose keys array had moved. With the descriptor holding
the edge there is nothing to reconcile — and nothing in the slot visitor reads
`keys_array` for a fact any more, which is what makes #8047 a deletion rather
than a rewrite. The header word is demoted to a derived mirror the collector
refreshes and keeps forwarding.

**Liveness is a real ephemeron gate, and it had to be.** The obvious
simplification — emit the descriptor edge once per TRACED receiver and let
per-object liveness be the ephemeron relation — is *wrong for a generational
minor*, and `PERRY_GC_VERIFY_EVACUATION` said so on the first end-to-end run:

```
old-young-edge-verifier failed: missing_edges=1
  parent=0x…7b8 type=object old_arena=true marked=false
  slot=0x…680 child=0x…308 child_type=array slot_page_ever_dirty=false
```

A minor never enumerates old objects, so an old carrier's edge is never emitted;
and the record is SHARED, so one sibling's rewrite creates an old→young edge for
a parent the minor never visits and which no per-parent remembered-set page can
describe. The gate is therefore per-descriptor state: `old_carrier`, set by the
slot visitor whenever it observes the shape on an old-generation receiver, and
consulted by `scan_shape_table_rekey_mut`, which roots exactly those records and
leaves the rest to metadata rewriting. A full trace enumerates every live object,
so `rotate_old_carrier_epoch_after_full_trace` recomputes the gate from what that
trace saw — the gate over-approximates by at most one full collection, which is
the generational contract, and never becomes unconditional rooting.

`gc/shape_keys_edge.rs` (its own file: `barrier/mod.rs` is at 1995 lines and
`cycle.rs` at 1991) answers the one question the old→young verifier has to ask
about a shared word — coverage for it is the shape table's root, not any
parent's page.

**Census first, per the issue's requirement (2) framing.** Peak descriptor
population over the 14 `gc_ratchet` probes plus a 1 MB JSON-parse kernel is
**~4 250 descriptors naming ~1 150 distinct keys arrays**, dominated by a fixed
bootstrap cohort; steady-state workload contribution is 1–229. So the protocol
is not a scaling problem, and the issue's requirement (1) — stable-address
descriptor storage — turns out to be **necessary after all**: the lift/write-back
the metadata pass already performs covers the *rewrite* only, never a strong,
rewritable edge.

**Validation.** `shape_churn.ts` (400 rounds × 400 records over 7 shapes, a
120-record retained window re-read by name after every collection window) under
`PERRY_GC_SCHEDULE_RATE` + `PERRY_GC_FORCE_EVACUATE` + `PERRY_GC_VERIFY_EVACUATION`
+ `PERRY_GC_PROTECT_FROMSPACE`, with the instruments proven armed
(`[gc-fromspace-protect] retired_set=#N`, `copied_objects` and
`promoted_objects` both non-zero over tens of thousands of copying minors).

`gc/tests/shape_keys_descriptor_edge.rs` adds four fixtures, all gating on
`copied_objects > 0` **and** on the receiver having actually moved:

* the descriptor record — not the header word — is enumerated as a child slot,
  and siblings of one shape share exactly one edge;
* with the header mirror suppressed, a keys array reachable only through the
  descriptor survives a copying minor and the record is rewritten to the live
  array (the #8047 rehearsal);
* **sabotage arm**: with the descriptor edge suppressed *as well*, the same
  workload leaves the record stale or pruned — so a green run of the previous
  fixture cannot be satisfied by something other than the edge under test;
* a keys array whose last carrier died is still reclaimed — an immortality bug
  is as real as a use-after-free.

The suppression switches are `#[cfg(test)]` thread-locals, not env knobs: the
GC-knob kill policy requires a required CI arm for every shipped knob's
off-state, and neither state may be reachable in a shipped binary.

`scripts/shape_descriptor_census.py` gains four authority surfaces and three
sabotage arms: un-boxing the record, un-gating the rooting arm, visiting the
derived mirror before the authoritative edge, and reading the mirror for a fact
are each red. The raw `keys_array` callsite count drops 183 → 180.
`object/shapes.rs` reached 2048 lines, so its three unit suites moved verbatim
to `object/shapes_tests.rs`.

### Fixed

- **A tombstone delete on a promoted receiver no longer loses the object's
  keys array under an evacuating collection** (#9200). On
  `test_gap_repsel_pshape_tower_delete.ts` with `PERRY_OBJECT_TOMBSTONES=1
  PERRY_GC_HEAP_LIMIT=8 PERRY_GC_FORCE_EVACUATE=1`, a deleted receiver came
  out of the churn with `Object.keys()` empty and a previously-live field
  reading `NaN` — silently, exit 0, stderr empty. This is the corruption that
  forced #9038's default-on tombstone deletes back to opt-in in #9212; the
  default flip is deliberately NOT part of this change.

  The mechanism, confirmed by tracing the descriptor lifecycle on the fixture
  (not by code reading — one earlier confident hypothesis had already failed
  against it):

  1. The delete's clone fork published an intermediate descriptor for the
     receiver's freshly cloned, nursery-young keys array and correctly armed
     its `old_carrier` gate (`set_object_keys_array_with_live` has done that
     since #8256 — the receiver had been promoted by the pre-delete churn, so
     no minor would ever enumerate it again).
  2. `publish_object_shape_holes` then minted the hole successor with
     `old_carrier: false`, stamped it, and **retired the armed intermediate**
     in its keys-address sweep (the #9064 descriptor-pile-up fix removes
     every other id under the owned keys address). Net effect: an old
     receiver stamped with an unarmed descriptor naming a young keys array.
  3. The next minor walks a non-carrier record metadata-only
     (`scan_shape_table_rekey_mut`) and never enumerates the old receiver,
     so the keys array had **no root at all**: it was swept while live,
     `prune_dead_shape_keys` dropped the descriptor as dead, and the
     receiver's stamp dangled. `object_keys_array()` resolves through the
     descriptor (#8047), so the receiver was shapeless from then on.
     `PERRY_GC_VERIFY_EVACUATION` cannot see any of this — the only edge
     lives in table metadata, and it is gone by sweep.

  The fix is structural, not a patched call site:
  `shapes::stamp_object_shape_id_with_carrier_note` is now the one post-birth
  publication point for a ShapeId into a receiver's header word — it stamps
  and, for any receiver outside the nursery, arms the descriptor's
  old-carrier gate in the same breath, mirroring the note
  `visit_gc_layout_slot_descriptors` makes at trace time. Every post-birth
  publish routes through it: `publish_object_shape_holes` (the bug),
  `try_update_stable_tombstone_shape`, `publish_object_shape_from`,
  `stamp_object_shape`, `birth_stamp_object_shape`,
  `transition_object_shape_semantics`, `transition_object_shape_to_class`,
  the reserved-floor stamp, and the plain cached-shape install (which
  hand-rolled the same note; the cache-carried install keeps its documented
  skip — `cache_carrier` is the stronger registration). The arming
  `set_object_keys_array_with_live` carried at its tail moved into the
  funnel. Over-arming costs one rooted record for at most one full trace —
  the epoch contract `old_carrier` already lives by (#8112).

  Affected files:

  - `crates/perry-runtime/src/object/shapes.rs` — the funnel, and the
    stamp sites in it.
  - `crates/perry-runtime/src/object/shapes_slot_list.rs` —
    `publish_object_shape_holes` / `try_update_stable_tombstone_shape` stamp
    through the funnel.
  - `crates/perry-runtime/src/object/mod.rs` — tail arming folded into the
    funnel.
  - `crates/perry-runtime/src/object/reserved_floor.rs` — floor stamp through
    the funnel.

  Validation: the tower fixture flag-on is byte-identical to node 26.5.1
  5/5 runs on `HEAP_LIMIT=8 + FORCE_EVACUATE`, 5/5 on the tighter
  `HEAP_LIMIT=4`, and 5/5 with `VERIFY_EVACUATION` added; the trace shows the
  hole descriptors as carriers with their keys arrays rewritten (evacuated
  live) instead of pruned. Flag-off is byte-identical to node before and
  after. `gc_repsel_matrix.sh --arms force_verify --filter tower_delete` with
  the flag exported: PASS with the arm live (moved-objects 1/1, copy-minor
  1/1). New witnesses: `test_gap_repsel_pshape_tombstone_oldgen_delete.ts`
  (the minimized non-tower trigger, registered in the corpus) and a unit pin
  (`tombstone_publish_on_untraced_receiver_arms_old_carrier`) that fails on
  the unfixed runtime and passes with the funnel.

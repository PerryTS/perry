### Fixed

- **`JSON.stringify`'s shape template no longer reads its element through a
  pre-collection address (#7268).** `try_emit_shape_element` derived the element
  header and its inline `fields_ptr` once, then looped over the fields calling
  `stringify_value_depth` for the pointer-valued ones — a call that can run a user
  `toJSON`, allocate, reach a safepoint and take an evacuating minor with it. Three
  holders were exposed: the bare `elem_ptr`/`fields_ptr` locals, the template's raw
  `keys_arr` (used as the shape identity *and* dereferenced to read the
  property-name strings back out), and `SHAPE_CACHE` itself — whose doc comment
  asserted the invariant that made the last two safe, *"within one top-level
  stringify call no GC runs over the user object graph"*. `toJSON` is user JS; the
  invariant is false, and the comment now says so instead of asserting it.

  All three are closed, because rooting any one leaves the others. In particular
  rooting the cache is **not** sufficient: `stringify_array_depth` builds its
  template into a plain `Option<ShapeTemplate>` Rust local that no scanner can see.
  So `try_emit_shape_element` roots the element (re-deriving `fields_ptr` on every
  access, the `cur_obj()` pattern `stringify_object_inner` already used) *and* roots
  `template.keys_arr`, writing the refreshed address back; and `SHAPE_CACHE` is
  visited by the already-registered `json_parse_mutable_root_scanner`, marked and
  rewritten on every frame.

  Two supporting changes each remove a way for the fix to be undone. `keys_arr`
  becomes a `Cell`: `try_emit_shape_element` holds `&ShapeTemplate` across the
  collection, and behind a shared reference the field is `noalias` and immutable to
  LLVM, so the post-call read could legally have been folded into the pre-call one
  — the fix would have compiled away. And reentrancy save/restore now pushes and
  pops a *frame* instead of `mem::take`ing the cache into a Rust local, where it sat
  outside the collector's reach for the whole of the inner call — the call that runs
  `toJSON`. The duplicated tuple key (`Vec<(*mut ArrayHeader, Box<ShapeTemplate>)>`
  stored the same pointer twice) is gone; one copy cannot drift from itself.

  Witness: `gc/tests/runtime_roots/json_shape_template.rs`, knob-free. A `Date`
  field is enough to reach the window — `stringify_value_depth`'s
  `is_date_cell_addr` branch calls `js_date_to_json`, which allocates — so no
  closure plumbing or `.ts` witness is needed. Probe shape
  `{ when: Date, answer: 42, also: Date }`, and the order is load-bearing.
  Sabotage-verified both halves; hoisting `fields_ptr` back above the loop produces
  `"answer":42.000010751275454` and the wrong ISO string for `also` — **silent data
  corruption, not a crash**, which is what shipped.

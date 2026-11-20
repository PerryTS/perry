#9869: `for-in`'s deferred shadow set recorded each walked prototype level as a plain
NaN-boxed `f64` in `VisitedLevels`, and dereferenced it later in
`build_shadow_set` → `mark_own_names` → `js_object_get_own_property_names`.

Between the `visited.push(current)` at level *N* and that read, the walk crosses
`js_object_keys_value` (which allocates an array) and
`js_object_get_prototype_of` (which can run a Proxy `getPrototypeOf` trap, i.e.
arbitrary user JS). Either can collect and move the recorded object, so the
stored word is a stale pointer whenever a collection lands in that window —
the same defect #9864 fixes for `out` and `current`, in the one place its patch
did not reach because the deferred-shadow-set rework landed after it was
written.

`VisitedLevels` now stores `RuntimeHandle`s, which the collector rewrites in
place, and `VisitedSlice::iter` reads each level fresh from its handle.
`RuntimeHandle` is `Copy`, so the inline arm still costs no allocation and the
"no malloc per `for-in`" property the rework was built for is preserved.

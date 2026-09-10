Potential next refinement, not implemented in frozen 46e9b34c6:

If post-materialization controls regress, route LazyArrayHeader.materialized
through the existing dynamic Array guard. ScalarProjection::emit would return
an additional (materialized handle, predecessor label), admitted only after
lazy magic/descriptors and nonnull materialized edge, plus pointed-to ordinary
Array brand and no forwarding. In inline_dyn_typed_array, object_array_guard
would select array_raw with a phi from original object_brand or that new edge.
Use array_raw throughout only the existing Array guard/load blocks. Bounds,
capacity, descriptor/prototype invalidation and holes retain the current guard.
All rejected cases still call the original boxed dispatcher on the lazy receiver;
forwarding is resolved there. This shares rather than clones the Array guard.

Also replace the new unit test's open-coded StringHeader payload offset with
crate::string::string_data before final validation/landing (one new lint finding).
Keep the current frozen build intact for an unambiguous R2 measurement first.

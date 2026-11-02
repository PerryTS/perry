Pay off the one raw-handle debt site this branch introduced.
`rule3_array_growth_to_the_shape_id_floor_is_refused` read the rooted array's
address out of its `RuntimeHandle` with a bare `get_raw_mut_ptr` and passed the
copy straight to `js_array_grow`, which
`scripts/raw_handle_debt.py` counts as debt in a module with no ceiling.

Converted to `handle.with_mut_ptr::<ArrayHeader, _>(|arr| …)` -- the scoped
form the ratchet exists to ask for, and the right one here because
`js_array_grow` is a runtime entry point that takes the current address as an
argument. No ceiling was added: the total returns to the recorded baseline of
906.

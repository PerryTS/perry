### Fixed

**`Array.prototype.reverse` now observes inherited, accessor-backed, and
deleted indices in specification order (#5898).** The raw-slot implementation
in `array::concat_reverse::js_array_reverse` treated holes as absent even when
an indexed prototype property filled them, and it observed both sides before a
getter could truncate the array. Exotic arrays now use live `HasProperty`,
`Get`, `Set`, and `Delete` operations while ordinary dense arrays retain the
allocation-free swap.

The carried receiver and values remain in runtime handles across accessors and
other collection points, with each value reloaded immediately before its set.
Array-length truncation in `array::push_pop::js_array_set_length` also deletes
descriptor-backed and sparse indices from high to low, and
`js_array_delete` clears a sparse side-table entry even after later growth has
made the index fit inside dense capacity. The regression in
`crates/perry/tests/issue_5898_array_reverse_exotic.rs` covers inherited
indices, getter-driven truncation, sparse truncation, and grow-then-delete.
Validation passes that compiled regression, all 221 `array::` runtime tests,
and all 50 repository lint gates.

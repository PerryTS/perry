Make canonical shape identity pass the merge-train custody and inventory gates.

Split basic object allocation and coercion into `object/alloc_basic.rs`,
with explicit named re-exports preserving both existing entry-point paths.
Canonical key allocation reloads its parent and appended key through
`RuntimeHandle::across_*`; key-list reads use scoped `with_const_ptr`.
`LiveObject::across` and `js_object_set_keys` likewise couple the receiver
reload to the operation that can collect. No raw-handle ceiling is added.

Canonical-array shared flags use the tracked-header resolver instead of
unchecked GcHeader casts. Keep existing birth-flag helpers in alloc.rs to
avoid adding ownership probes to unchanged object-construction paths. The canonical
trie's production TLS uses `perry_thread_local!`. Move canonical-key tests
and their slot-read counter into a cfg(test) module, using Perry TLS there
as well. The counter increments once per candidate slot in `probe`, reached
from `extend_slot` (via `extend_key` and `canonicalize`); no counter storage,
call or branch is compiled in production, including before this change.

Route both weak layout scanners through the existing object-cache scanner,
keeping the collector's pinned source unchanged from main and preserving
weak visitation before the other object caches. Name the slot extension
`extend_slot` to distinguish it from collection `extend` in the holder
checker's conservative name-based call graph; the previous collision falsely
made the regex EXEC_LOOKUPS test counter look scanner-covered. No holder
inventory or diagnostic-source pin is relaxed.

Refresh only the two changed object/mod.rs declarations using
`shape_descriptor_census.py --emit-baseline`: shape_cache_insert's former
single-line declaration becomes one additional `keys_array: *mut ArrayHeader,`
entry (2 -> 3) because it now accepts/returns LiveObject along with canonical
keys; test_shape_cache_insert's declaration now returns `*mut ArrayHeader`.
Callers must receive the canonical array, whose identity can differ from the
incoming private list. All other census entries and summary counts stay fixed.

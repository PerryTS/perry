**Fixed two move-sensitive bugs in the native yoga backend that crashed layout
under an evacuating collector.** Both sat in the measure path, and neither was
visible to the evacuation verifier, because neither offender is a slot the
collector walks.

`calculateLayout` copied every leaf's measure callback into a local
`HashMap<u32, f64>` while building the taffy tree. Those values are NaN-boxed JS
closures, and only the node registry's copy is rewritten when a cycle moves them
(`yoga_root_scanner` visits it with `visit_nanbox_f64_slot`); a plain local map
is visited by nothing. A measure callback re-enters JS and may allocate, so the
first leaf whose callback triggered a copying minor moved every closure and left
the snapshot pointing into from-space for every leaf measured after it. The
snapshot is gone: each leaf already carries its yoga handle as the taffy node
context, so callbacks are now resolved from the registry at measure time.

Separately, the code that reads `{width, height}` off the measure result held a
raw object pointer across the two key-string allocations it makes to read those
fields, so the `width` read could move the object and the `height` read would
dereference from-space. The result now lives in a temp root and the pointer is
re-derived after each allocation.

This also explains why the failure looked regime-dependent: in-place promotion
left the stale address valid, so it only surfaced once evacuating minors became
the steady behaviour.

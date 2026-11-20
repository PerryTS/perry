The native root scan resolves frame roots through the stack-map index, and an
index that was never built is indistinguishable downstream from an image that
genuinely has no native roots: both are empty. The consequences are not. With
statepoints as the only root mechanism, scanning against an unbuilt index means
the collector finds no roots on that frame, frees live objects, and corrupts the
heap with no diagnostic.

`visit_stack_map_root_slots` now asserts the property at the consuming end —
the same degradation `build_stack_map_index` already refuses at the producing
one. The invariant is stated as "an empty index means this image genuinely has
no native roots", not "initialize ran", which keeps the check live in every
configuration: `perry-runtime`'s unit tests reach the scan without `js_gc_init`
and pass because their harness carries no gc-map section — the right reason —
rather than by being exempted. Exempting them by build config would leave no
check in precisely the configuration where the index is legitimately unbuilt.

It cannot fire today, because `js_gc_init` builds the index eagerly before any
mutator code runs. It is here so that it CAN fire if the build is ever made
lazy and a path into the scan is missed, turning a silent delayed heap
corruption into a loud abort on the first test that reaches it. Landing it
while the build is still eager is deliberate: it proves the check sits on the
path it claims to guard before anything depends on it.

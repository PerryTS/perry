A generic property read site passes its receiver to the GC-leaf miss front
(`js_object_get_field_ic_front`) as the value its fused receiver test already
holds (the payload minus the native-handle floor, now one shared constant in
perry-abi), so the site pays a register move where it paid a 10-byte constant
and an add; the front adds the floor back inside its load displacements.
Every polymorphic way hit and latched megamorphic read saves three
instructions (lead_poly4 132.0 -> 129.7 instr/iter, lead_mega1 164.3 ->
161.5). The way cascade now asserts, in debug builds, that an
overflow-encoded slot never enters a way: the front answers a way with a
plain inline load and no spill re-test.

Inside an element-shape fast clone the element binding (`const o = a[i]`)
is virtual: it binds no slot, and every `o.field` read lowers through the
loop fact. For a class the whole-program facts prove (no subclass, fields
at fixed slots), the zero-guard read in `wpfacts_access::prove` claimed
`o.x` first and lowered the unbound `o`, so the loop threw "Cannot read
properties of undefined (reading 'x')" on its first iteration (competitive
C/object-get-1: a plain `for (i < a.length) { const o = a[i]; s += o.x }`).
`prove` now asks `element_shape_loop_fact_for_property_get` first, the
single entry point the other three sites already share.

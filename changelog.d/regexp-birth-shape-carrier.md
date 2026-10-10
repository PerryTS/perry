Keep the RegExp birth ShapeId stable for the agent's life (#12313). The
RegExp birth memo re-stamps one ShapeId with no receiver in between, so its
record is now an external carrier, as every other agent-lifetime intrinsic
birth memo's is (the base Function shapes, class and literal birth ids). A
full collection that finds no live RegExp no longer retires the record, and
the next birth no longer gets a new ShapeId, so compiled `lastIndex` reads
stay monomorphic in literal-churn loops.

The shape table now has one collector scanner for every heap word a shape
record holds: `scan_shape_table_rekey_mut` visits the canonical keys words and
the [[Prototype]] identity words, which were two registrations. The crash the
carrier change hit under forced evacuation came from a test root registry that
restored the keys scanner without the prototype-word scanner, so a pinned
record kept a from-space prototype. Production registered both; with one
scanner, no registry can restore one without the other.

Tests: the RegExp birth ShapeId and its record survive synchronous full
traces with no live RegExp (with a control that the trace retires an
uncarried shape), and the record's prototype word follows the intrinsic
prototype through forced evacuation, with an inherited read through it.

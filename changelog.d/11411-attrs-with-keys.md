Property attributes live with the keys (charter step 3, V8's descriptor
arrays). A key list that carries any non-default attribute (writable /
enumerable / configurable, accessor-ness) owns a parallel attributes array,
attached through the named-properties reserve slot the collector already
traces; attribute-free key lists are unchanged. The canonical key trie's edge
is `(key, attributes)`, so equal keys with equal attributes share one list and
one ShapeId whatever the path, and a key that arrives with its attributes
(an `exports` getter per re-export, a builtin prototype method, an arguments
object's `length`/`callee`) is appended in place. Each shape record carries an
attribute summary byte that the prototype-chain store check reads first.

An ordinary object's attributes no longer live in the address-keyed
`property_descriptors` table, its owner index or its meta-record Bloom bits,
and the attribute generation hash is gone. The read inline cache declines
only an accessor KEY and the write caches only a key that is not plain
writable data, so one `Object.defineProperty` no longer takes an object's
other keys off the property caches (#10871). The default-off
`PERRY_ATTR_DIAG` census (`--features perry-runtime/attr-census`) counts
which attribute paths a program runs.

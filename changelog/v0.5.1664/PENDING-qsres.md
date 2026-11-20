Object.keys now roots the shape's selected key backing across result allocation
instead of rooting the receiver and fetching its shape again, while retaining generic key-array resolution. Its fresh result
uses the existing fresh-list slot initialization helper, removing bulk-mutation
element-shape invalidation while retaining exact GC metadata, numeric
reclassification, and old-parent barriers. Enumeration ordering, filtering and
fresh result semantics remain covered by focused parity and protected GC tests.

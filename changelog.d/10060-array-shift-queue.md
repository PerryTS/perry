Make dense `Array.shift()` advance a logical front offset instead of moving
and rebuilding the GC layout of every survivor. The offset is derived from
the existing allocation size and remaining capacity, preserving the eight-byte
array header and clearing each removed slot immediately. Indexing, bulk
mutators, JSON, and GC tracing use the logical storage start; growth normalizes
the backing store and remembers copied young references.

Keep observable shifts on the property-aware path, including custom prototypes,
sealed/frozen arrays, and non-writable length. Check final length writability
after the indexed operations, preserving their side effects before an exception.
Add queue, moving-GC, and Node-parity regressions and a reproducible benchmark.

### Performance

Repeated `JSON.stringify` calls on the same small parser-produced record now
reuse a bounded native copy of the completed output after validating that the
receiver, property semantics, fields, and nested-array contents are unchanged.
The hit path validates every managed input before allocating the fresh result
string, so a collection during that allocation has no temporary input graph to
trace or rewrite.

The representative six-field record uses 61.97% fewer process instructions
than the pre-change runtime. Alternating receivers and changing values activate
a per-shape cooldown and add less than 0.8% in the measured miss workloads.

Added a direct-mapped cache in front of the shape-descriptor table.

`shape_descriptor_by_id` is on the hot property path. Profiling a dynamic
string-keyed property loop put it and `shape_descriptor_ensure_with_generation`
at roughly **13% of main-thread samples between them**, and every call paid a
TLS fetch, a `RefCell` borrow and a hash probe — to reach a record whose address
never moves. `Box<ShapeDescriptor>` is stable across rehash, so a 256-way
direct-mapped cache can hold the record's address and a hit becomes mask,
compare, deref. 4 KiB per thread, fixed.

Two decisions that matter for correctness:

**It caches the record's ADDRESS, not a copy of the descriptor.** Records are
mutated in place — `old_carrier`, `cache_carrier`, and `keys` after evacuation —
so a cached copy would go quietly stale. Holding the address means a hit always
reads current data.

**Epoch invalidation is selective.** Removal frees the box, and one insert path
can replace a live id with a fresh box; both bump the epoch. A *fresh-id* insert
deliberately does not, because it cannot invalidate an existing way, and bumping
there would flush the cache on every shape creation — exactly the workloads that
build shapes.

Measured with `benchmarks/bench_dynamic_property_keys.ts` on an idle host,
minimum of 7 runs:

| | delete-heavy | overwrite-only |
|---|---:|---:|
| node | 31 ms | 19 ms |
| before | 1222 ms | 1088 ms |
| after | **1187 ms** | **955 ms** |

Baseline dynamic property throughput improves **12.2%**. That is well short of
the 13% of samples the two shape functions hold, because only
`shape_descriptor_by_id` is served from the cache;
`shape_descriptor_ensure_with_generation` is a separate path and still probes.

This does not close the gap to node — perry is still ~50x on this loop. The
remaining weight sits in `js_array_get_f64` (324 samples) and
`value::addr_class::try_read_tracked_gc_header` (307), which are the next
targets and are not touched here.

The invalidation test is sabotage-checked: deleting the bump in
`remove_descriptor_and_reverse_indices` fails it. That check matters more than
usual here — a stale way is not a wrong answer, it is a dangling pointer to a
dropped `Box<ShapeDescriptor>` reached from the hot property path.

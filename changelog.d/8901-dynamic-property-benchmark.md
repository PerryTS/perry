Added `benchmarks/bench_dynamic_property_keys.ts`, and with it the measurement
that **refutes the premise for a dictionary mode**.

The phase-4 plan was a formal dictionary representation for objects that defeat
shapes — heavy `delete` use, thousands of unique keys — to stop pathological
objects minting shapes the table must then carry and rekey forever.

The benchmark runs two loops with the same number of property writes and reads,
differing only in a `delete`, so the ratio isolates shape churn from raw
property-access cost. Measured on an idle host, N = 300 000:

| engine | delete-heavy | overwrite-only | delete penalty |
|---|---:|---:|---:|
| node | 36 ms | 21 ms | **1.7×** |
| perry | 1487 ms | 1321 ms | **1.1×** |

**Perry's delete penalty is lower than node's.** Delete-driven shape churn is
not disproportionately expensive here, so a dictionary representation — a new
object representation, with its own property storage, PIC handling, enumeration
and GC integration — would be a large investment aimed at a tail perry does not
have. It is not implemented, and on this evidence should not be until a workload
shows the penalty that motivates it.

The second column is the finding worth acting on: **~60× on plain dynamic
property overwrite**. Profiling that binary attributes it to inline-cache misses
and shape-table probes, not deletion:

| samples | symbol |
|---:|---|
| 324 | `js_array_get_f64` |
| 307 | `value::addr_class::try_read_tracked_gc_header` |
| 105 | `shapes::shape_descriptor_by_id` |
| 103 | `shapes::shape_descriptor_ensure_with_generation` |
| 72 | `js_put_value_set_dyn_ic_miss` |

The two shape-table probes are ~13% of main-thread samples on their own — a
hash lookup per property access, on a key space (`ShapeId`) that is allocated
densely by a single atomic counter and could be an array index instead. That is
a concrete, bounded follow-up; it is not done here because the ids are minted
from a process-global counter while the tables are per-thread, so a dense `Vec`
could be sparse on a multi-threaded program, and that trade needs measuring
rather than assuming.

Both columns matter when changing this benchmark: the ratio is what refutes the
dictionary-mode premise, the absolute is what tracks the real gap.

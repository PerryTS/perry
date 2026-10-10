Extend statement-run read regions to pure arithmetic, logical and comparison initializers over up to two local receivers. A shared region guard covers up to five own inline keys per receiver, and its existing word now carries the shape's F64 lane bits. F64 leaves bypass tag checks; Any leaves require a Number proof before numeric operators run.

The generic arm retains each original initializer and its binding refinement. Missing keys, accessors, prototype changes and non-number values retain ordinary per-site semantics. Calls and potentially throwing environment reads end or refuse a region. Loop and store region encodings are unchanged.

Adds IR tests for guard sharing and disabled-emission negative control, a runtime word-layout test, and Node parity coverage for misses, NaN, representation changes and string refinement. Absent span reads in cli-table3 still need #10495 before this complete region can hit.

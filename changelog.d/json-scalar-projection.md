Probe own numeric, boolean and null fields directly in pristine lazy JSON array
records. Pure-local indexed reads retain ordinary property access on a miss;
escaped ambiguous keys, strings and containers fall back. One short ASCII field
can be memoized in the existing unexposed element slots; zero is encoded without
another allocation. Exposed records remain authoritative. GC continues to trace
only bitmap-selected elements, and collection policy is unchanged.
The probe sits on the dynamic index dispatcher's lazy-array edge. Ordinary
array/object reads keep their established branches. Long cold walks and wide
records fall back to ordinary access; repeated scalar hits use inline reads.
Mixed scalar and object reads retain adaptive batch construction.

Named descriptors and deletions now materialize lazy JSON arrays before applying
ordinary array semantics, preserving indexed getters, replacement values and
holes. Ordinary receiver definitions coerce keys once, before inspecting the
descriptor, with the inputs and converted key rooted across user callbacks.

This is an experimental R2 candidate following R1's rejected reuse regressions.
R2 preserves smaller-array scan improvements and improves repeated scalar reads,
but remaining CPU regressions and native-root checker findings prevent landing.
The measurements and validation limits are recorded in
`benchmarks/json_performance/SCALAR_MEMO.md`.

The subsequent R3 experiment routes a fully materialized lazy array into the
existing ordinary-array read guards after validating its backing array's brand
and forwarding state. Reads refresh the wrapper's cached length after mutations
through an alias. Growth, descriptors, holes and prototype invalidation retain
the established fallback. R3 improves smaller-array random and mixed-field reads,
but three measured CPU regressions keep it experimental and rejected for landing.
Actual main compilation reproduces both unsuppressed native-checker findings.
See `benchmarks/json_performance/SCALAR_MATERIALIZED.md` for all targeted rows,
validation evidence and the optimized dispatch-order investigation.

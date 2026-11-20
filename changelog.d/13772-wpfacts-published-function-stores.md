Per-unit facts now publish what each unit stores on a function value's own
properties. A store such as `k.parse = k` or `require.cache = ...` often runs
in a different unit from the function's owner, and the other units that held
the function never saw it: a call through `marked.parse(...)` read nothing and
its arguments did not escape, which made per-unit facts stronger than the
whole program. Every unit now publishes its stores per function name and key,
the driver joins them over all units, and every holder of the function (the
owner included) reads the join. Only unknown code makes the keys unknown.

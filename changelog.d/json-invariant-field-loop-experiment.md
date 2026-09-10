Experimental guarded reduction for repeated literal-index own-number field
reads. A non-collecting admission probe validates ordinary or lazy JSON array
data before a loop of sequential numeric additions. Zero-trip loops, accessors,
non-numbers, and unsupported receiver layouts retain ordinary expression
semantics. No GC policy, object layout, or managed-pointer cache is added.

The measured candidate remains unlanded: repeated reads beat Node and Bun, but
20m mixed fields regress 3.06% and tiny stringify regresses 0.67% against frozen
main. Full results, short-call controls, and validation limits are recorded in
`benchmarks/json_performance/INVARIANT_FIELD_LOOP.md`.

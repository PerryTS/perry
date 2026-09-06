Rejected experiment 8d587c86318bd1f1ae63284c23d7a0ef59d93fb8, reverted in 692a8c94f2d43506520d3db4b9ae10b2bd5016f5.

Unifies String/Inline native plans and outlines escaped emission. Generated
emit_piece shrinks 193 to 86 instructions and emit_record 863 to 818, but
string_piece grows 357 to 362 and local small-record instructions rise 0.68%.
A smaller emitter did not establish lower execution cost.

172 runtime tests, 38 compiled Node comparisons and 34 moving-GC runs pass.
Initial linking hit disk exhaustion after 22 comparisons; the other 16 failed
with errno 28. That attempt is preserved. Once 18 GiB was available, all 16 failed
links were retried with unchanged source/archives and passed. No user or other
agents’ files were deleted.

Full 152-verification/456-timing/432-memory and 700-trial paired matrices plus 24
extra retained-empty trials pass quiet gates with 32/42 clean observations.
Paired small-record stringify regresses 3.62%, escaped stringify 1.56%,
16 KB / 1 MB / 8 MB record-array parse 1.29%/1.55%/1.26%, heterogeneous parse 1.38%,
and tiny parse 0.68% vs escaped-count, with separated ranges. Empty parse
improves 2.05% and 16 KB array stringify 1.30%; these gains do not justify the
regressions. Against empty-parse, small-record stringify regresses 5.17%.
Full inventory remains 12/38 CPU, 58/74 peak RSS, 28/36 retained RSS.

The revert restores all 59 source hashes of the qualified escaped-count release.
GC production policy, thresholds, allocation paths and roots were not changed.
Array-root parsing may defer materialization; stringify inputs are materialized.
CPU includes default GC; RSS is whole-process. All-row acceptance remains open.

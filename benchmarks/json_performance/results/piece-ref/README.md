Experiment source: 96cda0524e5c8ce18834b1d05f7fe715ba2d8617.

Stringify length lookup and emission borrow Piece/Field plans instead of
passing them by value. The emitted code removes each array-element plan's
40-byte stack copy and the scalar emitter's second inline-byte temporary.
emit_piece shrinks from 78 to 76 instructions and its frame from 80 to 64 bytes.
The total emit_record instruction count remains 836: removing copies does not
establish a net instruction or CPU reduction. No production GC policy,
allocation path, rooting order or semantic fallback changes.

All 56 source hashes match. 165 JSON runtime tests and the root-holder gate
(existing 147 frontier unchanged) pass. The complete release passes all 37
compiled Node comparisons and 32 moving-GC runs.

A local orchestration mistake started diagnostics/validation before prepare
had returned terminal completion. The instruction diagnostic failed before
its first trial because the worker was missing; its log is preserved in
preparation-race. All first compiled comparisons happened to pass, but the
entire set was repeated after terminal preparation and verified archive hashes.
No attempted early transfer or code inspection succeeded. All remote worker
transfers and measurements use the verified final worker hash.

The full matrix passes its quiet gate with 32 clean external observations:
152 output verifications, 456 timing trials and 432 memory trials. The 616-trial paired run and 24 extra retained-empty trials pass their
quiet gate with 36 clean observations. The experiment is rejected and reverted
in 7eb11ae1d: paired record-array parsing is 4.99-5.53% slower, heterogeneous
parse 5.14% slower, and small-record stringify 1.27% slower, with separated
observed ranges. Tiny-object stringify improves 1.16%, escaped stringify
0.71%, wide stringify 1.90% and heterogeneous stringify 1.21%; these gains
do not justify the broad regressions. Full-run target inventory
is 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS at/below the better Node/Bun
median. Large-array parse is about 4-6% slower in this full run, while the
intended small-record stringify workload does not improve.

Local process instructions fall 1.26% for tiny-object stringify, but rise
0.16% for small-record stringify. These diagnostics are not accepted timing.
The current escaped-string sample has 2,222 main-thread samples, 2,018 entering
write_escaped_string, including 427 at its memcpy call. It supports direct
escaped-string output as a next experiment; see next-steps.md. No direct
escaped-output implementation is included here.

Array-root parse may defer materialization; stringify starts fully materialized.
CPU includes default GC and RSS is whole-process memory. This is not all-row
or no-regression acceptance.

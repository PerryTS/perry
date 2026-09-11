# Outlined object parsing: R42

Outlining the large object parser partially reduces the wide-object parse regression. R42 takes 2,977.0625 µs, 3.19% above its interleaved R26 reference; the previous R41 screen measured a 4.29% gap. R42 remains unpromoted: seven of the twelve targeted rows have separated CPU regressions versus R26.

One qualified quiet window contains 528 timed trials and 60 complete-output verification records, with eleven interleaved R26/Perry/Node/Bun repetitions per case. The twelve cases cover all ten regressions from the qualified R41 screen plus two long-string stringify controls. Every case was declared before R42 timing. Three additional rows were added to the initial nine-case declaration after R41 identified them; iterations and warmup remain unchanged.

All twelve candidate CPU medians are below both Node and Bun in this repeated-input screen. This does not establish general parity or fresh-input performance. Median peak-RSS changes versus R26 range from −32 to +96 KiB. CPU is user plus system time per loop iteration; peak RSS covers the complete process, including startup. Node is pinned to 26.5.1 and Bun to 1.3.14.

## Change and validation

The only implementation change is an inline(never) annotation and comment on parse_object_untyped. In the linked production worker, the SOURCE_LENGTH=true parse_value function shrinks from 1,760 to 576 instructions and the outlined object parser has 1,182 instructions. The false specialization retains its 117-instruction value dispatcher and 1,186-instruction object parser. These counts verify the intended call boundary; they are not a timing or causal proof. No parser algorithm, GC policy, roots, collection boundary, cache admission or cap changes.

Source b9bfe3070f90f223e1d9d3526df54beb57308328 passes 312 serial release JSON tests (2.91 seconds) and 385 overlapping string-filter tests (2.59 seconds). All 125 canonical candidate complete-output checks pass, with required positive moving/protected witnesses; 30 normalized native/shadow IR files and six worker objects match frozen R26. Original-suite checker verdicts reuse the ten actual R39 executions only after exact IR, full scripts-tree, source and log hashes match. Known native findings and reference behavioral failures remain explicit. The normal three-package production build completed in 444.39450335502625 seconds. Lint is 72/74: public benchmark freshness and the inherited raw-handle ceiling against pinned main f6c6879 remain; formatting, file cap and implementation audits pass. No checker, allowlist or baseline-ceiling edits. No alternate-worktree validation refusal occurred in this round.

Known native-root findings, lazy-descriptor and fractional-spacing findings, and frozen-reference emitter output/moving-GC failures remain unsuppressed and documented. The successful candidate checks do not erase those findings or constitute a blanket conformance pass.

## Targeted CPU screen

Window: 2026-09-11T18:03:32Z to 2026-09-11T18:06:23Z.

| Fixture / operation | R26 µs | R42 µs | Node µs | Bun µs | Change vs R26 | Screen |
|---|---:|---:|---:|---:|---:|---|
| records_array_16k / sparse | 20.013854 | 20.106762 | 39.307956 | 33.947255 | +0.46% | Overlap |
| records_object_1m / parse | 2012.728395 | 2027.493827 | 2852.222222 | 2151.790123 | +0.73% | Regression |
| records_object_8m / parse | 15922.600000 | 16111.400000 | 33895.500000 | 20927.900000 | +1.19% | Regression |
| records_array_20m / stringify | 11748.666667 | 11829.083333 | 17296.666667 | 20039.250000 | +0.68% | Regression |
| long_string_1m / stringify | 34.646202 | 34.323621 | 102.088970 | 95.095734 | -0.93% | Overlap |
| unicode_1m / stringify | 29.912323 | 28.824242 | 409.619798 | 448.454545 | -3.64% | Overlap |
| wide_1m / parse | 2885.015625 | 2977.062500 | 5275.703125 | 4138.437500 | +3.19% | Regression |
| wide_1m / stringify | 633.939655 | 648.396552 | 6361.512931 | 663.344828 | +2.28% | Regression |
| heterogeneous_1m / parse | 1118.936620 | 1124.492958 | 3919.492958 | 2947.021127 | +0.50% | Regression |
| tiny_object / parse | 0.036783 | 0.037073 | 0.081504 | 0.045199 | +0.79% | Regression |
| records_object_20m / stringify | 11717.833333 | 11789.083333 | 17263.083333 | 20086.833333 | +0.61% | Overlap |
| numbers_1m / stringify | 1013.040816 | 1019.836735 | 1975.904762 | 2942.727891 | +0.67% | Overlap |

“Separated” means the smallest candidate sample exceeds the largest reference sample. It is a descriptive screen, not a significance test; overlapping ranges do not prove equivalence. R42 has no second independent timing recheck.

## Peak RSS

| Fixture / operation | R26 MiB | R42 MiB | Node MiB | Bun MiB |
|---|---:|---:|---:|---:|
| records_array_16k / sparse | 72.234 | 72.234 | 66.031 | 70.656 |
| records_object_1m / parse | 66.438 | 66.469 | 92.984 | 79.172 |
| records_object_8m / parse | 187.328 | 187.328 | 244.297 | 131.297 |
| records_array_20m / stringify | 235.219 | 235.250 | 459.516 | 304.875 |
| long_string_1m / stringify | 54.516 | 54.484 | 183.656 | 153.703 |
| unicode_1m / stringify | 53.859 | 53.859 | 186.281 | 155.406 |
| wide_1m / parse | 227.703 | 227.703 | 121.188 | 95.094 |
| wide_1m / stringify | 68.641 | 68.734 | 117.969 | 84.359 |
| heterogeneous_1m / parse | 62.562 | 62.578 | 92.812 | 99.547 |
| tiny_object / parse | 32.281 | 32.281 | 59.609 | 69.078 |
| records_object_20m / stringify | 235.266 | 235.266 | 459.531 | 304.906 |
| numbers_1m / stringify | 57.844 | 57.875 | 113.078 | 74.281 |

## Immediate parent control

These R41 and R42 observations come from separate quiet windows, not interleaved R41/R42 pairs. R41 used seven repetitions per case and R42 used eleven. The parent window is historical evidence from [R41](https://github.com/PerryTS/perry/blob/b5bb7ecd299e9c9513964dcc423e3c33e46fa4d4/benchmarks/json_performance/PACKED_VECTOR_ESCAPE_R41.md), not an additional R42 trial count.

| Fixture / operation | R41 µs | R42 µs | Difference |
|---|---:|---:|---:|
| records_array_16k / sparse | 20.115658 | 20.106762 | -0.04% |
| records_object_1m / parse | 2025.308642 | 2027.493827 | +0.11% |
| records_object_8m / parse | 16148.100000 | 16111.400000 | -0.23% |
| records_array_20m / stringify | 11818.833333 | 11829.083333 | +0.09% |
| long_string_1m / stringify | 33.967742 | 34.323621 | +1.05% |
| unicode_1m / stringify | 30.244040 | 28.824242 | -4.69% |
| wide_1m / parse | 3013.828125 | 2977.062500 | -1.22% |
| wide_1m / stringify | 648.366379 | 648.396552 | +0.00% |
| heterogeneous_1m / parse | 1125.549296 | 1124.492958 | -0.09% |
| tiny_object / parse | 0.037128 | 0.037073 | -0.15% |
| records_object_20m / stringify | 11776.666667 | 11789.083333 | +0.11% |
| numbers_1m / stringify | 1021.000000 | 1019.836735 | -0.11% |

## Storage recovery and provenance

The first remote stage failed with “No space left on device” before any R42 benchmark ran. Under the owned benchmark lock, 66 previously recorded identical R26 reference binaries were replaced by hard links after complete hash and ownership verification. Every path, executable byte and benchmark output was preserved. Available disk space rose from 126,468,096 to 1,387,667,456 bytes. The retry verified all 150 staged hashes before timing. The failed transfer, exact replacement plan, before/after storage observations and per-file hashes are archived.

Candidate source: `b9bfe3070f90f223e1d9d3526df54beb57308328`. Immediate parent: `3952e832058a5e1efcd4cdf9cf28f26e31d60ea3`. Frozen R26 reference: `3aac4d6335da54abeeed73df842decbbe6dd5d71`. The index verifies every archived payload and Git blob; commands, immutable compiler/runtime/stdlib hashes, full verification records, complete CPU/RSS vectors and linked assembly are included. The timing window was archived before subsequent remote work.

No R42 full-50, fresh-input, Korean, stringify-options, retained-output or access-specific timing window ran. The existing R41 packed-escaping result belongs to its own measured source; this narrow parser screen does not remeasure it. R32 integer-remainder access improvements remain outside this experimental lineage.

A read-only integration check observed main at `435d6396c14a5e82ae8fb4cec2df9206aceb2a5d`: the zero-spacing PR #10052 landed through train #10082, and all three changed code/test files match its PR head. PRs #10064 and #10074 were open and ready. No CI waiting, administrative merge or current-main performance claim is involved.

The next experiment uses a bounded prefix check to reserve the long-token specialization for potentially useful documents. R42 alone does not meet the no-regression objective.

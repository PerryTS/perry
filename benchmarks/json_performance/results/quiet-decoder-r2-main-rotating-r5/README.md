# Complete R2 / merged-main changing-input comparison

Four engines, nineteen fixtures, three modes, five repetitions. All 380 output
comparisons and 1140 timing trials pass in the archived quiet window.
`baseline` = freshly rebuilt main e7223f700 (0.5.1528); `perry` = PR10034 decoder
R2 (0.5.1529). Node26.5.1 and Bun1.3.14 share the M1/8GiB host. Exact runner,
source patch, worker hashes, window and controller log are preserved.

All candidate/main parse CPU sample ranges overlap. The escaped-input
selection-only control has separated faster samples; it contains no JSON parse. The largest slower parse median
is +0.660% (same-source 20MiB object); among rotating inputs it is +0.377%
(small record array). Selection-only control medians vary up to +2.55%, also
with overlapping samples; those controls contain no JSON parse and are reported
without subtraction. Peak RSS does not increase; current RSS differs by at most
+0.015625 MiB. None of this is a guarantee of zero change on unmeasured workloads.

All CPU and RSS rows are in comparison.md; reference-screen.json includes every
candidate/main delta and sample list. merged-main-changing.json reports main's
own nineteen changing-input parse rows. The complete merged-main report explains
source reuse, the eight-input pool, and the null/empty-object content exceptions.

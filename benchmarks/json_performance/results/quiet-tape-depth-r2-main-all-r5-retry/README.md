# Tape-depth R2: qualified full original comparison

Quiet M1 window 2026-09-10 04:51:26–04:56:40 UTC; source/build
`df64624c8c7b08ca09e78f0006468f3d0befed67`, version 0.5.1530, versus freshly
built main `eee3881c464bf91ae900e87a42bed072bbdfc95a`, Node 26.5.1 and Bun
1.3.14. Five repetitions, equal work, default GC. All 200 correctness checks
and 344 measurement groups validate. The [earlier complete window](../quiet-tape-depth-r2-main-all-r5/README.md)
failed its final load gate and is retained without performance qualification;
no samples from it are selected into these medians.

Candidate and main both lead 46/50 CPU, 66/86 peak-RSS, and 31/36 retained-current
RSS targets against the better Node/Bun median in this window. The different
peak count from the previous window is not a new candidate-only target miss.
All remaining gaps are preserved in [the target inventory](parity.md).

Record-array parse improves 24.43% at 13 KiB, 23.53% at 1 MiB and 21.32% at
8 MiB versus main. Full scans improve 7.72%, 9.06% and 9.41%; sparse access
improves 16.28%, 22.54% and 20.27%; untouched roundtrip improves 16.69%, 16.82%
and 14.36%. Heterogeneous parse improves 20.06%. The 20 MiB direct-array control
has no positive CPU median delta in this run.

Heterogeneous stringify is +0.460% with fully separated slower CPU ranges.
Unicode same-source parse is +0.204%, consistent in direction with its focused
+0.249% concern. Unicode stringify is +2.748% here but -0.334% in the 27-repeat
focused replay, with overlapping ranges. Other small positive deltas remain
visible. No absence-of-regression or parity completion claim is made.

Maximum median peak/current RSS increases are 544/544 KiB, on the 13 KiB
roundtrip row. This is retained as a memory concern; unchanged collector source
alone does not prove identical measured memory. [All CPU/RSS rows](comparison.md),
raw samples, exact runners, quiet window, source/worker/reference provenance and
GC witnesses are archived. Repeated-source parsing may reuse cached values;
changing-source results are measured separately.

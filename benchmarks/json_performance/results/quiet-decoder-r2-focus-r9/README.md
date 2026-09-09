# R2 focused comparison against rebuilt main

Eighteen output comparisons against Node26.5.1 and 162 timing trials pass in the
saved quiet window. Six cases, nine repeats per engine, fixed work counts and
randomized three-arm order. `perry` = decoder R2, `baseline` = freshly built main
e7223f700, `prior` = decoder R1. Full raw CPU/RSS samples and per-process hardware
counters are preserved. This is a focused check, not the full Node/Bun standings.

Restoring the scalar scanner call boundary resolves the repeated R1 sparse-read
slowdown: R2 is +0.063% vs main and -2.38% vs R1, with overlapping main samples.
The other parse/scan medians are within +0.184% of main and also overlap. Long
ASCII stringify is -3.05% but remains variable, so this is not a claimed speedup.
Peak RSS is at most +0.046875 MiB (48 KiB). Full-matrix acceptance is still needed.

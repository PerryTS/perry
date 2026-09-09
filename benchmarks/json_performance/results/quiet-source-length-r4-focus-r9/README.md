# Source-length R4 focused comparison — experimental

Quiet M1 window 2026-09-09 19:12:05–19:13:45 UTC, nine trials per row.
All 80 output comparisons and 432 timing trials pass. Source341c99f7907326c2da7500a2091f39a3243fddcc, version0.5.1530; corrected R2 is
baseline. The exact source, binaries, runner and admission window are pinned.

Changing-input parse CPU improves 11.26% for ASCII and 45.29% for Unicode.
Small-record CPU is +0.225%, the other small object -0.306%; both overlap the
baseline samples. Selection-only controls have their own rows and are not
subtracted from parser measurements. Full CPU, peak RSS and current RSS are in
comparison.md, including unfavorable rows.

The [longer paired run](../quiet-source-length-r4-small-paired-r9/README.md)
leaves the small-record median +0.170% with overlapping samples. R4 remains
experimental; full-matrix acceptance is not claimed. The matched build, all
286 JSON tests, source gates and compiled GC/correctness proofs are in
[validation evidence](../source-length-r4-validation/README.md).

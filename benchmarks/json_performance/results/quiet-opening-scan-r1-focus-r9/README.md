# Opening-byte scan R1: focused comparison

Quiet M1 window 2026-09-09 19:39:17–19:41:01 UTC. Four engines, four fixtures,
three modes and nine trials per row. All 80 output comparisons and 432 timing
trials pass. Corrected R2 is the baseline; candidate source and worker hashes,
exact runner, default-GC witness and quiet admission are preserved.

Changing-input parse CPU is 7.63% lower for ASCII, 4.54% lower for Unicode and
3.37% lower for the roughly 1 KiB object, with separated sample ranges. Small
records have a 0.247% faster median with overlapping samples. Complete CPU and
RSS results, including selection-only and same-source controls, are in
comparison.md. Selection overhead is not subtracted.

This focused screen supports running the full original parse/stringify/RSS and
changing-input matrices. Full acceptance remains pending those checks.

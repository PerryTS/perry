# Opening-container scan R1: complete rotating suite

Candidate source `29236271f61a6cfb24de810ff4fe1f884c774c5e`, version 0.5.1530;
reference is corrected R2 from PR #10034. Quiet M1/8 GiB window:
2026-09-09 19:53:14–20:01:33 UTC. All 380 output comparisons and 1140 timing
trials passed (19 fixtures, three modes, four engines, five repetitions).

Each process retains eight preloaded inputs of equal size and shape. Seventeen
fixtures vary a value; null and empty-object fixtures use separately loaded
identical text. The same-source control retains the same input pool. Selection
is a separate control and is never subtracted from parsing time.

Selected changing-input results relative to corrected R2:

| Fixture | CPU change | Sample ranges |
|---|---:|---|
| null | -0.019% | overlap |
| string_a | -0.063% | overlap |
| empty_object | -0.082% | overlap |
| tiny_object | +0.372% | separated slower |
| small_record | -0.389% | separated faster |
| object_1k | -3.463% | separated faster |
| records_object_20m | -2.151% | separated faster |
| long_string_1m | -7.492% | separated faster |
| unicode_1m | -4.525% | separated faster |
| wide_1m | +0.173% | overlap |

R1 is not accepted as regression-free. Rotating tiny-object parsing is 0.372%
slower with separated ranges; the same-source small record is 0.174% slower
with separated ranges. The original worker also reported scalar regressions
that this rotating worker does not reproduce. A longer original-worker replay
checks those rows before the empty-allocation outlining follow-up is measured.

Maximum median peak/current RSS increases across these rows are 48/64 KiB.
The scanner does not address the large-container retention issue documented in
`../../GC_MEMORY_GROWTH.md`.

[All CPU and RSS rows](comparison.md), [all paired samples](reference-screen.json),
[quiet admission](window.json), and exact raw JSONL, runners, source patch,
artifact hashes and GC witnesses are preserved in this directory.

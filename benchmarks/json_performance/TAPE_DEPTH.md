# Reuse tape depth admission

Eligible arrays previously paid a nesting preflight followed by the native tape
builder's syntax pass. The tape stack now records depth as it validates the
input. Shallow arrays keep lazy results; deep valid trees materialize
iteratively. Rejected tapes receive the legacy depth/error checks before any
recursive fallback. The 1000-level recursive limit, 500000-level resource budget
and auto-tape 16 MiB ceiling are unchanged. Direct parses and forced oversized
tapes retain their preflight. GC policy remains unchanged.

R1 (`f55569a565581fc964a73b063213774b4601d4e1`) has completed the focused, original and
changing-input suites against freshly built main `eee3881c4`. R2
(`df64624c8c7b08ca09e78f0006468f3d0befed67`) keeps both tape-builder
specializations out of the parsing function. LLVM confirms that change;
R2 performance measurement is pending. Do not transfer R1 timings to R2.

## R1 measurements

All 200 original-suite output checks and 344 measurement groups pass on the
quiet M1, Node 26.5.1 and Bun 1.3.14, with five repetitions and equal work.
Record-array parse improves 14–16%, sparse access 12–15%, full scan 5–6% and
untouched roundtrip 10–11% over main across the 13 KiB, 1 MiB and 8 MiB rows.
The 20 MiB direct-parser control changes little. Maximum median peak/current
RSS increases are 160/96 KiB. The CPU/RSS parity counts remain 46/50, 67/86 and
31/36; the remaining gaps are still visible in the complete inventory.

The nine-repeat focused run also shows array gains. Its Unicode stringify
median is 7.03% slower (7/9 paired repetitions slower), while the original suite
shows 4.32% faster Unicode stringify at a different work count. This conflicting
evidence remains unresolved. The original suite also has small positive ASCII/
Unicode parse and wide-object deltas; overlapping ranges do not establish
absence of regression. No acceptance or complete parity claim is made.

- [Full original CPU and RSS table](results/quiet-tape-depth-r1-main-all-r5/comparison.md)
- [Every Node/Bun target](results/quiet-tape-depth-r1-main-all-r5/parity.md)
- [Longer-count, three-arm focused comparison](results/quiet-tape-depth-r1-main-focus-r9/README.md)

The full changing-input suite passes 380 output checks and 1140 trials. Arrays
improve 12–15%; the Unicode same-source parse control is +0.698% with fully
separated slower ranges. This remains an unresolved concern.

[All changing-input, same-source and selection CPU/RSS rows](results/quiet-tape-depth-r1-main-rotating-r5/comparison.md).

## Validation and generated code

Both source revisions pass all 291 JSON release tests single-threaded and their
matched compiler/runtime-static/stdlib-static builds. Nine compiled depth
witnesses cover auto/tape/direct modes under normal, scheduled-moving and full
GC; all outputs match Node. Each scheduled depth arm protects 2772 retired sets
and moves more than 46000 objects. The existing protected scan moves 208531
objects through 1323 minors; retained results and escaped-record corrections
remain covered. Recurring stringify malloc counts match main at 39,40,40,40.

R1 inlines native construction into `parse_slow`, increasing its symbol span to
13784 bytes and its stack allocation to 560 bytes. R2 emits separate tape
builders and reduces that function to 5624/512 bytes. Before tape-depth fusion,
opening-scan R3 was 5416/496 bytes. This verifies the code change, not its speed.

- [R1 source, tests, compiled/GC witnesses and assembly](results/tape-depth-r1-validation/README.md)
- [R2 source, tests, compiled/GC witnesses and assembly](results/tape-depth-r2-validation/README.md)

This builds on [opening-scan and dispatch work](OPENING_SCAN.md). Large-container
retention still needs separate work described in [memory diagnostics](GC_MEMORY_GROWTH.md).

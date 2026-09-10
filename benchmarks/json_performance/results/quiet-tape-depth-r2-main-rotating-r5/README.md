# Tape-depth R2: all changing-input rows

Quiet M1 window 2026-09-10 04:58:54–05:07:21 UTC; source/build
`df64624c8c7b08ca09e78f0006468f3d0befed67`, version 0.5.1530, versus freshly
built main `eee3881c464bf91ae900e87a42bed072bbdfc95a`, Node 26.5.1 and Bun
1.3.14. All 380 output comparisons and 1140 timing trials pass. Five repetitions,
equal work and default GC. The eight preloaded equal-size same-shape inputs
change one value except null/empty-object contents. Same-source and selection
controls retain that pool; selection costs are never subtracted.

All nineteen changing-input CPU medians improve over main in this window:
record arrays are -23.75% at 13 KiB, -22.05% at 1 MiB and -22.36% at 8 MiB;
heterogeneous arrays -19.90%. Small-record/object_1k are -0.77%/-3.15%, and
ASCII/Unicode string objects -7.57%/-4.61%. These are descriptive medians,
not a proof of significance for every small delta.

One same-source control has separated slower ranges: null +0.135%, about
0.000016 microseconds per call. It is retained in the report. No other mode has
fully separated slower CPU ranges. The maximum median peak/current RSS
increases are 48/80 KiB. This does not clear the heterogeneous stringify,
Unicode cached-parse or roundtrip-memory concerns from the other windows.

[All CPU/RSS rows](comparison.md), raw samples, exact runners, quiet window,
source/worker/reference provenance and GC witnesses are archived. No complete
parity or absence-of-regression claim is made.

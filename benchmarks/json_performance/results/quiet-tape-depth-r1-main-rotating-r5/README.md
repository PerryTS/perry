# Tape depth R1: all changing-input rows

Quiet M1 window 2026-09-09 22:15:54–22:24:18 UTC; source/build
`f55569a565581fc964a73b063213774b4601d4e1`, version 0.5.1530, versus freshly
built main `eee3881c464bf91ae900e87a42bed072bbdfc95a` (0.5.1529), Node 26.5.1,
and Bun 1.3.14. All 380 output comparisons and 1140 timing trials pass. Five
repetitions, equal work and default GC. Eight preloaded equal-size same-shape
sources change one value (except null/empty-object contents); the same-source
and selection-only controls retain that identical pool. No control subtraction.

Changing-input record-array parse medians improve 14.98% at 13 KiB, 13.80% at
1 MiB and 13.92% at 8 MiB versus main; heterogeneous arrays improve 11.97%.
Small-record parse changes -0.04%, object_1k -3.11%, ASCII-string parse -7.41%
and Unicode-string parse -4.80%. Wide-object parse is +0.13%, with overlapping
ranges. All nineteen rows and all same/select controls are retained.

One control has fully separated slower CPU ranges: Unicode same-source parse
+0.698%. No other row is separated slower. This remains a regression concern;
it is not erased by the changing-source improvement. Maximum median peak/current
RSS increases are 128/144 KiB. No no-regression or parity acceptance is claimed.

[All CPU and RSS rows](comparison.md), raw trials, exact runners, quiet window,
source/worker/reference hashes and GC witnesses are archived.

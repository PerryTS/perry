Direct-output scalar plans now format exact finite integer f64 values below
2^53 through itoa. Both signed zeros emit 0. Fractions and larger magnitudes
retain ryu-js shortest-roundtrip spelling, and existing NaN/Infinity/boxed-value
handling remains unchanged. This adds no allocation, handles or GC interaction.

Source: 64b5df4b008f746b88b0c99c99ee784fe241df09.
152 JSON runtime tests pass, including both the existing random numeric oracle
and a new ryu-js comparison across signed power-of-two neighbors through 2^63,
fractional values, signed zero and 16,384 signed exact-integer pairs.
All 35 compiled Node comparisons and 28 moving-GC runs pass.
The full matrix is qualified: 152 verifications, 456 timing trials and 432
memory trials with 32 clean observations. The separate paired run covers all
38 CPU rows with 532 trials and 34 clean observations. Both quiet gates pass.
All 49 recorded source hashes match the measured source commit.

Against record-output, paired tiny-object stringify improves 30.20%
(0.13656 to 0.09533 microseconds), small-record stringify 21.59%
(0.40287 to 0.31587), and 1 KB object stringify 14.40% (0.28632 to 0.24509).
Inline-string stringify improves 3.12%, recovering the preceding increase.
16 KB record-array stringify improves 1.76%, 8 MB array/object stringify
0.61%/0.48%, and heterogeneous parse 2.26%, with separated observed ranges.

No-regression acceptance fails: empty/tiny/small/1 KB object parsing is
3.39%/2.87%/1.79%/4.53% slower; null/inline-string parsing is 2.33%/1.93% slower;
long-ASCII/Unicode parsing is 5.41%/3.35% slower; and large record parsing has
smaller 0.39–0.83% increases, all with separated paired ranges. Heterogeneous
stringify is 0.52% slower. Escaped stringify's 2.29% lower median has overlapping
ranges, so this run does not firmly establish recovery of its older regression.

Small peak-RSS increases remain: typically 0.03–0.14 MiB in object/array rows.
The older 8 MB lazy-parse peak-RSS regression also remains (about 102.6 MiB).
The target inventory stays 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS.
See tables.md, parity.md and recheck-integer-piece/summary.json for all rows.

The parse source is unchanged by this candidate. Five inspected parser
functions have equal instruction counts and mnemonic streams; four local
GC diagnostic pairs have matching old scans, copying minors and final arena
live/capacity. Neither finding proves equal execution cost. Do not dismiss
repeated CPU regressions as noise or attribute them to GC without more evidence.
See next-steps.md for the ownership-transfer tape experiment and remaining work.

# Decoder correction R1: correctness passes, performance not accepted

The six paired cases passed 12 Perry/reference output comparisons against Node
and 108 timing checks in a quiet admitted window. Nine repetitions per engine
use identical fixed work counts and randomized arm order. The raw instructions
and cycles include process work and are divided by the timed iteration count;
no selection or startup cost is subtracted.

Small record-array sparse reads were 25.75362 vs 25.08838 microseconds per call
(+2.65%). Eight of nine candidate samples exceed all nine reference samples;
one candidate sample overlaps. This repeats the initial full-matrix slowdown
(+1.31%), so R1 is not accepted as regression-free. Retired instructions decrease
slightly while cycles increase, which does not excuse the measured slowdown.

Small-array parse and scan shift +0.59% / +0.36%. The escaped-1MiB parse,
record-array-1MiB parse, and long-ASCII stringify shifts do not replicate
(-0.008%, -0.24%, -0.20%). Peak RSS differs by at most 0.03125 MiB.

The next candidate will explicitly retain the tape scanner call boundary. R1
had inlined that scanner into materialize_string_value after removal of the old
large duplicate escape decoder (484 -> 1120 bytes); the new canonical decoder
and correctness coverage stay intact. This is a hypothesis to measure.

The reference is the original PR10032 head b2111219c. Main subsequently landed
its complete implementation through train PR10033 at e7223f700, plus a test-only
batch witness repair. The next measurements will use a fresh build of that merge.

The ordinary parse entry now roots its input before servicing pending collection
and rederives the bytes after that hook. The lazy tape route uses that same
input root and completes collection before borrowing the payload for scanning.
GC production policy and thresholds are unchanged.

The new forced-collection tests expose the preceding bounded-preflight source:
valid direct-object and lazy-array inputs both exit with a SyntaxError after
nursery evacuation poisons the original source. The result-returning entry is
a passing control. Apply pending-input-witness.patch.gz to the preceding commit
to reproduce those tests. These are forced allocation-point collection tests;
they do not claim a failure occurred in the default benchmark matrix.

All four input-lifetime tests now pass, asserting actual collection and movement
and comparing the result with the original text. One covers the separate tape
entry trigger with no pending debt. The complete JSON runtime filter passes
144 tests. The root-holder inventory gate passes with its existing 147
unverified frontier holders unchanged.

Container calls branch before the outlined scalar decoder, avoiding its large
register frame. The depth byte-bound proof moves to the entry wrapper, leaving
the large scanner body unchanged from primitive-object. The serializer again
admits one-field primitive nested objects, in addition to wide objects. Other
complex objects retain the generic closure scan and callback path.

The added compiled fixture retains lazy outputs, follows malformed tape input
with valid parses, and checks nested one-field values, getters, toJSON and
spacing. The same application object matches Node with the preceding runtime.
Release validation passes 34 compiled Node comparisons and 26 moving-GC runs.

Measured source: a7fda79684c60f79311e4c19d5aabf0436f7772e

The full matrix is qualified: 152 verifications, 456 timing trials, 432 memory
trials and 32 clean external observations. The first paired admission rejected
load above 2.5 before any trials; its failure evidence is preserved. The retry
is qualified: 616 trials across 44 cases and 42 clean external observations.

Against bounded-preflight, paired heterogeneous stringify improves 4.85%
(4596.21 to 4373.35 microseconds), tiny-object parse 1.12%, small-record parse
0.95%, 16 KB record-array stringify 1.56%, 1/8/20 MB record-object parse
1.01%/0.83%/0.82%, and 20 MB record-array parse 0.85%, with separated ranges.
Against primitive-object, heterogeneous stringify improves 3.37%, recovering
its preceding regression, and wide-object stringify improves 7.28%.

No-regression acceptance still fails. Null and inline-string parsing regress
2.31% and 2.05%; escaped stringify regresses 2.52%; 1 MB record-array parsing
regresses 0.23%, all with separated paired ranges. Large parsing still trails
the older primitive-object checkpoint: 8/20 MB record-object parse is
1.26%/1.63% slower, and 20 MB record-array parse is 1.63% slower. The 1 MB
record-object difference is 0.93% with overlapping ranges. Numeric parsing's
full-matrix 1.59% slowdown shrinks to 0.51% with overlapping paired ranges.

Peak RSS also regresses in the paired runs: 1 MB record-array parse rises
4.73 MiB (62.16 to 66.89 MiB), and heterogeneous parse rises 3.77 MiB
(68.05 to 71.81 MiB), with separated ranges. Local GC diagnostics show equal
old-reclaim scan counts and final arena capacities; they do not explain the
whole-process peaks or prove equal GC time. Tape construction/collection order
is an isolation experiment to run next, preserving the corrected input root.

Target inventory remains 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS.
This checkpoint is not all-row or no-regression acceptance. Revisit the scalar
entry split, escaped-string emission and tape scratch lifetime; the direct
final-output path for small records remains a larger next performance change.

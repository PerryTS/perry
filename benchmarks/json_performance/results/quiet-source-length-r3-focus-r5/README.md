# Source-length R3 focused experiment — not accepted

Exact source: 1a3f5b3943541a40434b0af2daa7baa9dbf28214, package 0.5.1530.
Later rebased onto validation-document changes only; the measured source patch,
binary hashes and generated code are preserved here. Reference is the corrected
R2 binary from PR #10034, not the unfixed merged-main decoder.

Quiet M1 window: 2026-09-09 18:54:56–18:55:56 UTC. 80 output comparisons and
240 timing trials pass, with five repetitions, four fixtures, four engines and
three modes. The complete CPU and RSS table is in comparison.md.

Changing-input ASCII-string parse CPU improves 11.25%, Unicode 45.26%.
The small-record median is 0.49% slower, with every candidate sample slower than
every reference sample. The other small object is 0.08% faster with overlapping
samples. Peak/current RSS shifts are at most 96/64 KiB in this focused run.
R3 is not accepted under the no-regression requirement; no full matrix is run.

Outlining the ordinary constructor restores parse_string_value's 96-byte frame
(1004 bytes of code versus reference 976); the previous source-length experiment
had a 144-byte frame and 2408 bytes. Inspection also shows the factored allocation
body remains a separate call from the ordinary constructor. The next experiment
will inline that body into its two constructors while keeping the ordinary
constructor and large-source proof outside parse_string_value.

All 286 release JSON tests, matched compiler/runtime/stdlib build and existing
source gates pass. GC witnesses match the reference: 1323 protected copying
minors in scan, retained-output correctness under normal/scheduled/full GC,
26 ordinary minors in 1000 changing Unicode parses, and unchanged recurring
malloc-sweep counts 39,40,40,40. These are separately collected correctness
witnesses; they are not timing measurements.

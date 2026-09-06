Bounded ordinary records write directly into the final exact-size string.
The path accepts at most eight fields and sixteen primitive-array elements in
total. Stack plans carry only lengths, indices and inline scalar bytes. The
parent root traces the keys, arrays and strings, which are rederived after
prototype initialization/final output allocation. Escapes, undefined/holes,
complex fields, descriptors, named array properties, custom prototypes,
reordered keys and oversized plans decline to the generic serializer.

The tape route restores build-before-collection allocation ordering through a
raw-input scratch API. Its byte borrow ends before the callback can collect,
and it rederives input from the root afterward. The pending-hook rooting fix
is preserved. The deep route uses the same borrow-safe API. No GC production
policy or threshold change is made.

The new compiled fixture exposed a preceding compact-array bug: index getters
were ignored by raw fallback slot reads. A separate descriptor/prototype-aware
fallback now performs ordered Get operations and snapshots the original length.
It roots the array and callback results, and avoids speculative getter reads
for shape-template construction. The preceding Node difference is preserved.

151 JSON runtime tests pass. Forced-collection tests assert that the direct
record path actually runs, and that parent/array/string child pointers move at
both cold prototype lookup and output allocation. Another test invokes an
array getter exactly once, forces a minor inside it, and checks later elements
after the array moves. The raw-tape test releases the source in its callback.

Source candidate: 53052fc09d47d956e8decf22f677a773f94d42cb
Release validation passes 35 compiled Node comparisons and 28 moving-GC runs.
The full M1 matrix passes 152 verifications, with 456 timing trials and 432
memory trials. All 33 external observations are clean and the quiet gate passes.
All 49 source hashes match the measured provenance.

Latest inventory: 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS medians at or
below the better Node/Bun median. See cpu-38.md for all CPU rows, tables.md for
CPU and RSS, and parity.md for every target. The full-matrix small-record
stringify median falls from 0.524 to 0.402 microseconds. The paired checks below do not establish no-regression acceptance.

The paired run is now qualified: 686 trials across 49 cases, with 45 clean
external observations and a passing quiet gate. Against parse-entry, small
record stringify improves 23.14%, tiny-object stringify 2.73%, empty-object
parse 1.76%, and tiny-object parse 0.79%, with separated observed ranges.

No-regression acceptance fails. Against parse-entry, inline-string stringify
regresses 3.22%, empty stringify 0.96%, numeric stringify 2.19%, wide stringify
1.12%, heterogeneous stringify 0.35%, 16 KB/1 MB/8 MB array parsing
1.68%/1.41%/1.75%, heterogeneous parsing 3.50%, and 20 MB object parsing 0.40%,
with separated ranges. Other rows and their ranges remain in the raw evidence.

The tape allocation-order change recovers the preceding small lazy-parse
peak-RSS regressions: 1 MB record-array parsing falls 4.86 MiB, and heterogeneous
parsing falls 3.88 MiB. Both are 0.09 MiB below bounded-preflight. However, 8 MB
record-array parsing rises 14.31 MiB (88.17 to 102.48 MiB), with separated ranges.
Restoring one allocation order does not resolve every memory target.

Against bounded-preflight, small-record stringify improves 23.40%, heterogeneous
stringify 4.44%, and empty/tiny parse 1.76%/1.80%. Inline-string parse remains
2.03% slower, escaped stringify 2.40% slower, and heterogeneous parse 3.66%
slower, with separated ranges. These results do not erase older regressions.

The local instruction diagnostic records 27.57% fewer process instructions for
small-record stringify. Its stack sample identifies floating formatting of
integer-valued fields as a concrete next cost. See local-instructions/ and
small-record-profile/ for diagnostics and their limitations.

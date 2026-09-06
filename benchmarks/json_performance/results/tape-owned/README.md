Large completed native tapes now transfer into lazy-array side storage through
an exact-size boxed slice. An already exact-sized Vec keeps its allocation;
shrinking excess capacity may reallocate. Small scratch buffers retain the
existing copy-and-reuse path. Input borrows end before the callback, and the
blob remains rooted across pressure accounting and header/cache allocation.
GC production policy, thresholds, owner generation and finalization stay intact.

The compiled retained-output fixture exposed a preceding bug: dynamic index
assignment treated a lazy array as an ordinary object and silently lost writes.
The lazy-only mutation path now roots receiver, key and value, materializes,
then applies the normal computed setter with its original strictness. It also
updates the lazy alias's inline length after extension or truncation. Numeric
and string-key dispatch reach this path; normal receiver handling is preserved.

Source: cc1c4f582c29c5bc12e54bf15f6f81a1226ad857.
161 JSON runtime tests and five dynamic-index tests pass. Tests prove exact
storage transfer, exact accounting and release, independent retained owners,
full-collection ownership, teardown, malformed-input recovery, and actual blob/
key/value movement during construction or mutation. The root-holder gate passes
with its existing 147 unverified frontier holders unchanged.

The preceding-mutation witness is compiled with the pinned compiler and linked
to integer-piece's matched runtime. It exits successfully but serializes the
old zero element after assignment, producing the wrong complete-output checks
and checksum. Its Node comparison is preserved; the object SHA is recorded.
Release validation passes all 36 compiled Node comparisons and 30 moving-GC
runs (15 fixtures, seeds 17/9013), with actual copying and object movement.
All 54 source hashes match the measured commit. Full output validation agrees
across all four engines in all 38 cases (152 checks). The 456 timing and 432
memory trials pass their quiet gate with 32 clean external observations.

The first 574-trial paired run failed the ending load gate (2.620 > 2.5), despite
35 clean external observations. It is preserved in recheck-unqualified-1 and
no subset is accepted. A complete 574-trial repeat passes with 35 clean
observations, covering all 38 CPU rows against integer-piece plus three parse
rows against the older parse-entry memory anchor.

Against integer-piece, paired 8 MB record-array parse CPU improves 1.81% and
peak RSS falls 3.28125 MiB (102.640625 to 99.359375). Retaining four 8 MB results
uses about 2.89 MiB less current RSS in the full matrix. Long ASCII / Unicode
parse improves 5.12% / 2.76%, 1 KB object parse 2.93%, and numeric stringify
2.50%, with separated paired ranges. Other changes remain in the complete data.

Regressions remain: inline-string parse +5.71%, 1 MB record-array parse +2.38%,
numeric parse +0.86%, and heterogeneous stringify +0.69%, with separated ranges.
Heterogeneous parse CPU ranges overlap, but its peak RSS rises 1.21875 MiB;
numeric parse peak rises 0.328125 MiB. The 8 MB parse peak remains 11.203125 MiB
above the older parse-entry anchor. Ownership transfer is therefore only a
partial memory recovery. Local instruction diagnostics show about 1.27-1.66%
fewer instructions for selected tape parses; these are not accepted CPU timings.

The current median inventory is 12/38 CPU, 58/74 peak RSS and 28/36 retained
RSS at or below the better Node/Bun median. Escaped stringify's 0.3% full-run
advantage is effectively a tie. Median comparisons do not prove significance.
The all-row and no-regression goal remains open. Array-root parse may defer
materialization; stringify inputs are fully materialized. CPU includes default
GC, and RSS is whole-process memory.

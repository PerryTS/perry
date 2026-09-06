Measured source 7117f9e67d11af140ee857880b9a96af2ed2890e. This remains a
development checkpoint; all-row and no-regression acceptance are false.

Bounded word packing now applies only to complete 4–7 byte strings in scalar
output planning. Seven shared scanner function bodies match escaped-count;
general parse and serializer tails are restored. The native record plans use
initialized String placeholders with smaller active payloads. Final managed
output allocation, input rooting/rederivation and GC production policy remain
unchanged. Byte/oracle, adjacent-byte, alignment and guard-page coverage checks
the new short-string predicate directly.

172 runtime tests, 38 compiled Node comparisons with identical pinned fixture
objects, 34 moving-GC runs and all 59 source hashes pass. Matched four-package
release settings and immutable archives are verified. The full 152-verification
/ 456-timing / 432-memory matrix and 756-trial paired matrix (54 cases), plus
24 retained-empty trials, pass quiet gates with 32/44 clean observations. The
paired matrix includes all 38 rows versus escaped-count, older empty/entry/tape
anchors, and four direct comparisons to the shared-tail experiment.

Paired small-record stringify improves 10.47% versus escaped-count, from
0.320849 to 0.287247 microseconds, with peak RSS down 0.078125 MiB; both ranges
are separated. Full CPU is 0.285303 microseconds versus Node 0.109152 and Bun
0.120765, so Perry still uses 2.61x Node's CPU. Paired empty/tiny/small/1 KB
object parse improve 3.80%/2.20%/1.45%/1.48%, with separated ranges. All four
also improve versus empty-parse, recovering the earlier measured regressions.
Small-record stringify is 9.14% faster than empty-parse. Tiny-object stringify
improves 1.12% versus escaped-count; versus empty-parse its -0.07% median has
overlapping ranges.

Confirmed paired regressions versus escaped-count remain: escaped stringify
+1.55%, wide stringify +0.99%, and 20 MB object-root parse +0.48%. Numeric parse
is +1.12% by median with overlapping ranges; it remains +1.60% versus tape-owned
with separated ranges. Long-ASCII/Unicode parse are +0.65%/+0.23%, overlapping.
The older heterogeneous-parse regression is +2.31% versus parse-entry. All prior
unresolved regressions remain requirements.

Versus short-tail, small-record stringify is +0.07% with overlapping ranges;
Unicode parse improves 2.45% with separated ranges. Long-ASCII parse improves
3.66% by median with overlapping ranges. The shared-tail 1 MB array-parse gain
is lost: +1.63% versus that experimental worker, with separated ranges. The
current worker is -0.22% versus escaped-count on that row, overlapping. These
comparisons show both retained gains and costs of restricting the scanner.

Paired inline-string stringify peak RSS rises 0.015625 MiB, with separated
ranges. Retaining 200k empty objects uses 24.71875 MiB versus reference 24.625,
Node 83.484375 and Bun 50.21875 MiB: +0.09375 MiB versus Perry's reference.
The older 8 MB array-parse peak regression remains +11.265625 MiB versus
parse-entry. The full 20 MB array-stringify peak is +2.15625 MiB, while its
same-call paired comparison is -0.125 MiB with separated ranges; the full-run
increase is not established as stable. Every raw result is retained.

Target inventory remains 12/38 CPU, 58/74 peak RSS and 28/36 retained RSS. Extra
retained-empty trials do not change the original targets. Array-root parse may
defer materialization; stringify inputs are fully materialized. CPU includes
default GC; RSS is whole-process. Median comparisons and observed ranges are
reported separately and do not by themselves establish statistical significance.

Generated emit_record shrinks 863 to 809 instructions; string_piece grows 357
to 393. Local small-record stringify process instructions fall 7.03%, while
numeric parse, escaped stringify and wide stringify are nearly flat. Their
M1 CPU results still differ, so instruction counts do not establish speed.
The large-record profile supplies a separate next-step diagnosis; see
large-record-profile/README.md and gc-coordination.md.

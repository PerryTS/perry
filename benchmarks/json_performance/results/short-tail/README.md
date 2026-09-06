Measured experiment 090e120f3773e78aa699bcf87a12eb15d38ec3b3. This is a
validated development checkpoint; no-regression acceptance fails.

Four-to-seven-byte scanner tails use bounded little-endian loads and inert
space padding. First-match arithmetic is checked against independent scalar
oracles, every byte and adjacent-byte pair, alignment and guard-page cases.
Zero-valued String placeholders initialize the native record output plans.
Generated code replaces the scalar tail loop; the smaller active placeholder
payload reduces initialization stores rather than clearing all plan storage.
The record emitter shrinks from 863 to 809 instructions (flat object 724 to
713); string_piece grows from 357 to 391. Local small-record stringify process
instructions fall 6.06%, but numeric parse rises 1.73%. These are diagnostics,
not accepted local CPU measurements.

172 runtime tests, 38 compiled Node comparisons using the same pinned fixture
objects, and 34 moving-GC runs pass. All 59 source hashes, immutable worker and
runtime archives, and matched release settings are verified. GC production
policy and the implementations of the parse-boundary hooks remain unchanged
from v0.5.1520. JSON-local rooting and leaf call-sequence changes are documented
in the branch report.

The full 152-verification / 456-timing / 432-memory matrix and the 700-trial
paired matrix plus 24 retained-empty trials qualify, with 32/42 clean external
observations and both quiet gates passing. Target inventory is unchanged:
12/38 CPU, 58/74 peak RSS, 28/36 retained RSS. Array-root parse may defer
materialization; stringify inputs are fully materialized. CPU includes default
GC, and RSS is whole-process memory. Ratios and acceptance are median-based;
separated observed ranges are reported separately.

Versus escaped-count, paired small-record stringify improves 9.82%, from
0.321198 to 0.289653 microseconds, with separated ranges. The full matrix is
0.287808 microseconds versus Node 0.108602 and Bun 0.120713, so Perry still uses
2.65x Node's CPU. Paired empty parse improves 4.64%, escaped parse 3.35%,
heterogeneous parse/stringify 2.42%/2.48%, record-array parse 1.57–1.98% at
16 KB–8 MB, and record-array stringify 1.12–2.49% at those sizes. The earlier
small-record stringify regression versus empty-parse is recovered: -8.48%.

Confirmed paired regressions versus escaped-count: long-ASCII parse +3.69%,
1 KB object parse +1.51%, escaped stringify +1.36%, wide stringify +0.87%,
numeric parse +0.69%, tiny-object parse +0.68%, and 1 MB object-root parse
+0.29%. Unicode parse is +3.21% by median with overlapping ranges. Small-record
parse is +0.33% with overlapping ranges; it is +1.10% versus empty-parse with
separated ranges. Tiny and 1 KB object parse remain +1.39%/+2.03% versus empty.
No other earlier unresolved regression is declared closed by these results.

Paired scalar peak RSS rises 0.03125 MiB for null/string parse and
0.0625/0.078125 MiB for null/string stringify, with separated ranges. The full
numeric-stringify peak rises 0.515625 MiB; the same-call paired comparison
instead falls 0.03125 MiB, so that full-run rise is not established as stable.
Retaining 200k empty objects uses 24.828125 MiB versus reference 24.625,
Node 83.53125 and Bun 50.1875 MiB: +0.203125 MiB versus Perry's reference.
The older 8 MB array-parse peak regression remains +11.359375 MiB versus
parse-entry. Extra retained-empty trials do not change the target inventory.

The shared scanner change reaches general parse and stringify paths. The next
experiment restores those callers and limits word packing to short strings in
the scalar output planner, retaining the initialized native plan seeds. Its
purpose is to test whether the measured small-record benefit survives without
these shared-path regressions. This is a hypothesis until that next build has
its own complete comparisons.

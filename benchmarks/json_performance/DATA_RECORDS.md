The current source adds allocation-free scalar parsing and a guarded record
stringify path. **This is a development checkpoint. Full CPU/RSS parity and
no-regression acceptance remain open. The current empty-object release has
qualified full-matrix and paired measurements; it reaches 11/38 CPU, 58/74 peak RSS and
28/36 retained RSS targets.**

A subsequent change reuses the record preflight when emitting primitive-array
children. It passes 133 runtime tests, 31 compiled Node comparisons and 20
moving-GC stress runs, and removes about 5.5% of local process instructions on
16 KB/1 MB records. Its first benchmark admission was rejected before taking
measurements because a foreign worker had started. Its second full run was
unqualified because foreign workers started midway. Its implementation is
included in the current qualified empty-object release below.
[Validation-reuse implementation and evidence](results/record-array-proof/README.md).

The current empty-object follow-up returns its inline output before field
planning and temporary rooting, using only the probe that cannot allocate.
Cold prototype lookup declines to the rooted general serializer. It passes
135 runtime tests, 32 compiled Node comparisons and 22 moving-GC stress runs;
the cold-empty test explicitly asserts input relocation. The full matrix has
152 verifications, 456 timing trials and 432 memory trials with 32 clean
external observations. A separate 210-trial paired check has 13 clean
observations; both windows pass their load and competing-process gates.
[Empty-object release evidence](results/empty-object-leaf/README.md).

Against the record release, the paired current build reduces empty-object
stringify CPU by 11.08%, 16 KB record-array stringify by 4.79%, and 1/8/20 MB
record-object stringify by 4.00%/3.53%/1.27%. Escaped parsing is 0.65% slower
and 1 MB object-root parsing is 0.31% slower, with separated paired ranges.
Numeric parsing's apparent 1.43% full-matrix regression does not reproduce in
the paired check; the older regression versus Unicode remains unresolved.
[Current 38 CPU rows and RAM](results/empty-object-leaf/tables.md),
[every target](results/empty-object-leaf/parity.md), and
[paired ranges](results/empty-object-leaf/recheck-empty-object-leaf/summary.json).
Current measured source is f7bc8848770d5b03422478de6e44384feeae6015;
all 40 recorded source hashes match it.

A subsequent ARM mask-search experiment passed correctness but regressed
escaped parsing by 50.63% and record-array parsing by 5.89–9.51% in a qualified
full matrix. It was reverted. LLVM already removed the original source-level
mask store; replacing the early lane exits with bit counting reduced code size
without improving execution. [Rejected experiment](results/neon-mask/README.md).

Successful inline scalar parses decode before the existing pending-collection
hook. They skip the ordinary parser's suppression/rebaseline cycle while
preserving pending GC debt and the oversized parse-key cache/ring cleanup.
Allocating parses keep the existing rooted flow in separate slow functions.
The initial scalar candidate measured 11/38 CPU, 58/74 peak RSS and 28/36 retained
RSS medians at or below the better Node/Bun median, but review found its missing
cache cleanup and paired checks confirmed small-object parse regressions.
That initial source and its measurements are retained in
[parse-scalar](results/parse-scalar/README.md). Both full runs of the corrected
scalar candidate overlapped foreign ccperf workers and failed the load gate;
[those runs](results/parse-scalar-cache/validation.json) are unqualified.
Source patches reproduce every recorded source hash for both candidates.

The record emitter targets ordinary inline records containing primitives and
dense primitive arrays. A shape hint avoids speculative walks for records with
nested object fields; every eligible element is validated before output. The
successful walk uses bounded stack snapshots and a Rust output buffer, with no
managed allocation, user callbacks, intermediate managed roots or collection.
Overflow layouts, forwarded array heads, descriptors, named array properties,
holes, complex children,
BigInt, prototype uncertainty and depth limits retain the general traversal.
The first default-prototype lookup can initialize globalThis and allocate its
lookup key, so the record path declines that cold lookup until the rooted
serializer has populated the prototype cache.

Two correctness findings accompany this work. The existing shape-template path
ignored a later record's non-enumerable property or getter; it now checks each
receiver's descriptor bit before raw-slot emission. The earlier direct-output
small-object path rooted its input after the allocating first prototype probe;
it now roots before that probe. A forced-moving-GC test crashes with SIGSEGV
under the old placement and passes under the corrected placement, explicitly
asserting that the input moved. This is not a non-moving or empty GC witness.

All 133 runtime tests selected by the JSON filter pass. New record-leaf tests
check exact output and 1,000 emissions without managed growth, extra handles,
collection or stringify-stack changes; complex children and the cold prototype
probe decline before output. The root-holder inventory passes with its previous
unverified frontier unchanged. The record release passes all 31 compiled Node comparisons and all 20 moving-GC
stress runs (10 subjects, seeds 17/9013), with verified copying and object
movement. The previously failing getter/enumerability witness now matches Node.
The measured source is df32314ace602869c8a75584981a0e7bd0d0f7f0; all 39 source
hashes match that commit. The full run passes its load/competing-process gate
and 35 external observations are clean: 152 output verifications, 456 timing
trials and 432 memory trials. A separate 252-trial paired check has 20 clean
observations and its own passing gate. [Preceding record validation](results/data-record/validation.json).

The paired CPU results below compare this record release with the preceding
tape-scanner release, except where explicitly labeled Unicode. CPU includes
user and system time with default GC. Ranges are from seven fresh processes per
arm; a separated range is evidence of a repeatable difference in this run, not
a general confidence interval.

| Workload | Reference µs | Record release µs | Change | Ranges |
|---|---:|---:|---:|---|
| Small record stringify | 0.549 | 0.531 | -3.40% | separated |
| 16 KB record array stringify | 31.724 | 24.986 | -21.24% | separated |
| 1 MB record object stringify | 1,985.307 | 1,614.744 | -18.67% | separated |
| 8 MB record object stringify | 16,843.100 | 13,933.800 | -17.27% | separated |
| 20 MB record object stringify | 94,352.000 | 87,257.500 | -7.52% | separated |
| Empty object stringify | 0.05145 | 0.05565 | +8.15% | separated |
| Tiny object stringify | 0.13977 | 0.14104 | +0.91% | separated |
| Tiny object parse | 0.36009 | 0.36657 | +1.80% | separated |
| 1 KB object parse | 0.58502 | 0.59148 | +1.10% | separated |
| Small record parse | 0.74338 | 0.74850 | +0.69% | separated |
| Numeric parse, versus Unicode | 1,573.615 | 1,700.000 | +8.03% | separated |
| Heterogeneous parse, versus Unicode | 1,869.885 | 1,918.179 | +2.58% | separated |
| Heterogeneous stringify, versus Unicode | 4,491.335 | 4,542.305 | +1.13% | separated |
| Wide stringify, versus Unicode | 2,467.940 | 2,478.472 | +0.43% | separated |
| Escaped stringify, versus Unicode | 1,862.217 | 1,862.349 | +0.01% | overlap |

The prior escaped-stringify regression does not reproduce in this paired run.
The empty-object stringify cost accompanies the required early rooting fix;
the unsafe old root placement is not an acceptable performance baseline to
restore. [All 38 CPU rows and memory tables](results/data-record/tables.md),
[all target comparisons](results/data-record/parity.md), and
[all 18 paired checks](results/data-record/recheck-data-record/summary.json)
retain the other results rather than hiding them in an average.

On a busy local development host, an independent instruction diagnostic shows
about 19% fewer process instructions for 1 MB records but only 6% fewer for
20 MB. A subsequent 20 MB stack sample has 1,452 of 2,167 main-thread samples
entering GC from the benchmark loop's safepoint, versus 696 entering stringify.
This supports GC as a major contributor to the 20 MB cost; it is not an exact
GC CPU percentage or an accepted speed measurement. JSON's final output copy
accounts for only 37 samples. The follow-up described above removes repeated
primitive-array validation inside the already validated record walk.
[Local diagnostics and limitations](results/data-record/README.md).

A separate local parse diagnostic records only 0.23% more instructions for
numeric parsing than Unicode, despite its qualified 8.03% CPU regression.
All three instrumented builds report nine old-allocation reclamation scans,
8 MiB ending arena capacity and roughly 3.35 MB live bytes for the same 128
calls. This does not prove equal GC time, but it does not support attributing
the whole slowdown to extra parser instructions or more of those GC scans.
Tiny-object parse instead retires 2.33% more instructions than tape, directing
attention to scalar eligibility and wrapper overhead on the container path.
[Parse counters and diagnostic traces](results/data-record/local-parse-diagnostic/README.md).

GC production sources, trigger policy and gc_bump_malloc_trigger remain unchanged
from v0.5.1520 (454daac4f). parse_api.rs deliberately changes: the scalar decode
runs before the existing pending-collection hook; ordinary allocating parses
retain their previous suppression, trigger and parse-boundary scheduling flow.
The GC-file diff contains tests only. No version bump is included yet.

The preceding tape-scanner results remain in [TAPE_SCANNING.md](TAPE_SCANNING.md).
Neither that report nor these changes establish the requested all-row result.
Array-root parse can defer materialization; stringify
starts fully materialized. CPU includes default GC; RSS is whole-process memory.

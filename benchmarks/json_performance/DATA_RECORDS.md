The current source adds allocation-free scalar parsing and a guarded record
stringify path. **This is a development checkpoint. Full CPU/RSS parity and
no-regression acceptance remain open; the record candidate has no accepted
performance measurements yet.**

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
unverified frontier unchanged. The preceding corrected scalar build passes 30
compiled Node comparisons and 18 moving-GC stress runs. The new record release
still needs its 31 compiled comparisons, 20 moving-GC stress runs and qualified
CPU/RSS measurements. [Current validation evidence](results/data-record/validation.json).

GC production sources, trigger policy and gc_bump_malloc_trigger remain unchanged
from v0.5.1520 (454daac4f). parse_api.rs deliberately changes: the scalar decode
runs before the existing pending-collection hook; ordinary allocating parses
retain their previous suppression, trigger and parse-boundary scheduling flow.
The GC-file diff contains tests only. No version bump is included yet.

The last fully qualified committed tape-scanner results remain in
[TAPE_SCANNING.md](TAPE_SCANNING.md). They include unresolved regressions against
the Unicode checkpoint; neither that report nor these changes establish the
requested all-row result. Array-root parse can defer materialization; stringify
starts fully materialized. CPU includes default GC; RSS is whole-process memory.

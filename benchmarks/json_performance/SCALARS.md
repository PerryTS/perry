# Inline scalar results and compact decimal formatting

`JSON.stringify` now returns null, booleans and eligible short ASCII strings
using Perry's existing five-byte inline-string representation. This entry path
only runs with inert replacer and spacer arguments. It performs no heap
allocation, invokes no callbacks and retains no scratch state. Other results
of at most five ASCII bytes also use an inline return value after the regular
serializer has completed its work.

The numeric emitter adds a guarded path for finite, non-integer values whose
shortest decimal spelling has at most three fractional digits. It checks both
the scaled integer and the exact binary roundtrip before writing. Values
outside the proven range, and those that fail either check, retain the Ryu-JS
path. Both paths use stack storage and append to the existing output buffer.

## Measured results

The quiet Apple M1 comparison uses Node 26.5.1 and Bun 1.3.14, with default
GC and three fresh processes per row. CPU per call:

| Stringify case | Unicode reference | Scalar candidate | Node | Bun |
|---|---:|---:|---:|---:|
| null | 65.887 ns | 6.878 ns | 27.206 ns | 28.713 ns |
| "a" | 74.395 ns | 10.009 ns | 30.102 ns | 29.494 ns |
| Small record | 550.081 ns | 566.584 ns | 109.137 ns | 120.637 ns |
| Numeric array | 6.603 ms | 2.256 ms | 1.935 ms | 2.951 ms |
| 1 MB record envelope | 2.201 ms | 2.077 ms | 0.844 ms | 0.968 ms |

Null and `"a"` stringify improve 9.6× and 7.4×, resolving the previous
tiny-scalar regression and beating both engines. Numeric-array stringify
improves 2.9×; it beats Bun but still takes 17% more CPU than Node.
General record stringify at 16 KB–8 MB improves about 6–8%. Across all
measured peak/current RSS rows, no candidate median increases by more than
0.16 MiB. RSS remains whole-process memory, not temporary JSON allocations.

**This checkpoint still fails the no-regression requirement.**
[Seven-process repeats](results/scalar/recheck-scalar/summary.json) confirm
3.1% slower small-record stringify, 2.5% slower numeric-array parse, and
2.5% slower escaped-string stringify. Their baseline/candidate timing ranges
do not overlap. These losses must be recovered before performance acceptance.

The [full matrix](results/scalar/tables.md) and [target inventory](results/scalar/parity.md)
have 8/38 CPU medians, 58/74 peak RSS medians and 28/36 retained RSS medians
at or below the better engine. These counts are not a statistical or
correctness completion gate.

## Validation

- 51 JSON runtime tests pass, including all ASCII two-byte combinations for
  short-string escaping and compact-decimal guards and adjacent floats.
- 25 compiled fixtures match Node. The new fixture retains 2,600 results,
  reads their code units, parses them back, and exercises replacer callbacks,
  observable and throwing spacer coercions, and reentrant `toJSON` calls.
- 1,730,283 exact double spellings match both Node 26.5.1 and Bun 1.3.14.
  The harness imports the production emitter and observes zero temporary
  allocations with output capacity already reserved. The corpus includes
  680,154 additional compact-decimal and adjacent-float cases.
- Four retained-output fixtures pass moving-GC stress with seeds 17/9013,
  from-space protection and evacuation verification. All eight executions
  have positive copying-collection and moved-object counters.
- GC sources and `json/parse_api.rs` remain unchanged from the v0.5.1520 base.
  The production numeric emitter also compiles for x86-64 Linux; the local
  execution and performance results are ARM-only.

## Measurement conditions

The comparison uses the Unicode runtime at `cb8d8f251` as its reference, with
identical application object code, matched release settings and default GC.
The compiler is the same pinned development artifact used throughout this
study; this measures a rebuilt runtime, not a freshly rebuilt compiler.

The accepted measurement window passed the load gate and recorded no external
benchmark/compiler activity. All 152 output checks pass; 456 timing and 432
memory trials complete. The seven-process follow-ups also pass the quiet gate.

The first two full runs failed the host-load gate and remain preserved under
[`results/scalar-unqualified`](results/scalar-unqualified/) and
[`results/scalar-unqualified-repeat`](results/scalar-unqualified-repeat/).
The second run's monitor caught an external benchmark starting during our lock
and consuming about 120 CPU seconds. Those two runs are excluded from the
performance conclusions above.

[Validation](results/scalar/validation.json),
[number-corpus checks](results/scalar/number-validation.json), and
[source/archive provenance](results/scalar/provenance.json) identify the tested
candidate. The [local profiles](results/scalar/profiles-local/README.md) are
sampling diagnostics on the busy development Mac, separate from standings.
They place 1,564/2,249 main-call-tree samples for the 20 MB record stringify
case in the loop GC safepoint, predominantly full collection and live-graph
tracing. The 1 MB case instead centers on traversal, field access and string
emission. Full CPU/RSS parity and the no-regression goal remain open.

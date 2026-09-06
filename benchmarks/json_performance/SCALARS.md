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

**Performance acceptance is pending.** Both full runs passed all 152 output
checks and completed 456 timing and 432 memory trials each, but failed the
host-load gate. They are preserved under
[`results/scalar-unqualified`](results/scalar-unqualified/) and
[`results/scalar-unqualified-repeat`](results/scalar-unqualified-repeat/).
The second run's process monitor caught an external benchmark starting during
our lock and consuming about 120 CPU seconds. Both runs are excluded from
performance conclusions; a quiet rerun must also check the smaller apparent
regressions, not merely confirm the large gains.

The last qualified Node/Bun comparison remains the
[Unicode runtime matrix](results/unicode/tables.md), with
[all CPU and RSS targets](results/unicode/parity.md). The previous tiny-call
regression is not declared resolved until a qualified repeat proves it.

[Validation](results/scalar/validation.json),
[number-corpus checks](results/scalar/number-validation.json), and
[source/archive provenance](results/scalar/provenance.json) identify the tested
candidate. The [local profiles](results/scalar/profiles-local/README.md) are
sampling diagnostics on the busy development Mac, separate from standings.
They place 1,564/2,249 main-call-tree samples for the 20 MB record stringify
case in the loop GC safepoint, predominantly full collection and live-graph
tracing. The 1 MB case instead centers on traversal, field access and string
emission. Full CPU/RSS parity and the no-regression goal remain open.

# JSON construction batch experiment

This experiment implements construction batching inside the existing eager
parser. The comparison parent is `76bc51e8ff933384ab21a3bf5088df5c0d9fcbd6`
(`record-bytes`). It is isolated on `codex/json-batch-builder-1520`; the pending
array `toJSON` changes remain in the `codex-json-fastpaths` worktree.

## Construction contract

The parse API already roots its input, performs pending collection, suppresses
collection during construction, roots the result, then restores scheduling and
accounts for allocation pressure. Those boundary calls remain unchanged.

Within that window the new builder:

- Reuses the ordinary arena's inline cursor, avoiding repeated TLS resolution
  and arena synchronization for successful nursery allocations. Regular key
  allocation and block refill share that cursor and cannot overlap its objects.
- Writes final record fields once, then installs their final tracing state.
  Shape identity still goes through the existing mint-and-stamp helpers.
- Accumulates array layout facts in native variables. Numeric arrays stay
  pointer-free; pointer-only arrays receive the all-pointer flag; mixed arrays
  build their selective mask once from the completed payload.
- Publishes large parents' old-to-young edges after filling them. The existing
  remembered set records slot pages: one existing slot barrier for a young
  child on a page covers that page. Pages containing only scalar or old children
  stay clean. There is no new remembered-set representation.
- Copies unescaped strings into final ordinary String storage. Escaped strings
  retain the existing WTF-8 builder/canonicalization path.

There is no second document representation, reserved slab, ownership wrapper,
or retained per-parse allocation list. Small arrays retain the existing bounded
native prefix; wide records retain the existing temporary vectors and key index.
Every output object has its normal header, identity, mutation behavior, and
individual lifetime. Array growth leaves ordinary unreachable prior allocations
for the existing collector, with their layouts and remembered edges complete.

Allocation during incremental marking retains ordinary black-birth seeding and
per-store shading. The batch is declined for that phase. This is deliberately
different from suppressing collection and then forgetting its marking protocol.

Malformed input completes any constructed container before returning failure.
Native scratch is released normally; unpublished managed garbage remains subject
to the existing allocation-debt and collection policy. The implementation does
not rewind the heap across shared key/shape-cache writes.

## Scope and limits

Ordinary eager parse and the result-returning parse API use the batch. Scalar,
empty/inline-object leaves, lazy tape roots, deep iterative materialization and
typed shaped records keep their separate existing paths. Default array-root
benchmarks may still measure lazy parse; stringify benchmarks eagerly materialize
their input before timing, as in the parent comparison.

This change does not alter stringify's traversal or callback behavior. Stringify
still needs its own work; both operations remain in the performance matrix,
because their input layouts and linked code can affect their measurements.

Graphs retained through collection still need tracing. Deferring collection
until a transient graph dies can avoid tracing or copying it, depending on the
reclamation path. Batching construction alone does not change that scheduling
or promise constant-time collection, zero allocation, or lower retained memory.
The wide fixture is one object with 50,000 properties; the new construction test
also explicitly exercises an array containing 50,000 distinct records.

## Validation

- 213 runtime JSON tests pass, including five new construction tests: 50,000
  records with actual child movement; retaining one child without copying its
  999 siblings; mixed-array growth and malformed input; allocate-black fallback;
  and a 20,000-field old object with sparse young children across slot pages.
- The existing GC root-holder inventory passes. Production collector policy,
  thresholds, marking/copying implementation, `gc_bump_malloc_trigger`, and
  parse-boundary hook implementations are unchanged from the parent. The new
  arena construction path is an explicit allocation integration change.
- The repository file-size gate reports two unchanged parent violations:
  `crates/perry-codegen/src/inprocess.rs` (2,416 lines) and
  `crates/perry-hir/src/lower/tests.rs` (2,009 lines).

42 full compiled Node comparisons and 44 moving-GC runs are complete. A quiet
four-engine matrix and 38 qualified seven-pair comparisons confirm substantial
parse gains, with remaining CPU/RSS regressions. See [the measured results](results/batch-builder/README.md)
and [the GC timing explanation](results/batch-builder/gc-boundary.md).
All-row parity, no regressions, and complete semantics remain unachieved.

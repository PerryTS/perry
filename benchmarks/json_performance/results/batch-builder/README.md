# Eager JSON construction batch

Measured source: `e9de6a967eb660e6aa7957aa95bc8535708966d6`, branch
`codex/json-batch-builder-1520`. Reference: record-bytes, source `2103f419`.
The same pinned application object is linked against matched runtime/stdlib
builds. The array `toJSON` changes remain isolated in the other worktree.

The qualified four-engine comparison reaches **16/38 CPU, 65/74 peak RSS and
33/36 retained RSS targets**. The previous Perry build reaches 13/38 CPU targets
in the same window. This is an experimental improvement with remaining
regressions, not all-row or no-regression acceptance.

## Results

The qualified seven-pair repeats confirm these CPU changes against record-bytes:

| Parse workload | CPU change |
|---|---:|
| Small record | −19.01% |
| 1 KB object | −8.64% |
| 1 MB record object | −27.98% |
| 8 MB record object | −29.54% |
| 20 MB record array | −29.17% |
| 20 MB record object | −29.65% |
| Wide object | −2.21% |

All these observed ranges are separated. In the full comparison, the 8 MB
record-object parse and both 20 MB parse rows newly beat both Node and Bun.
See [all 38 CPU rows](cpu-38.md), [all CPU/RSS targets](parity.md), and
[all 38 paired comparisons](paired/table.md).

Adverse results remain visible. Qualified pairs show separated CPU increases
for 1 KB stringify (+1.925%), small-record stringify (+0.970%), heterogeneous
parse (+1.127%), heterogeneous stringify (+0.839%), and tiny-object parse
(+0.640%). Tiny-object stringify rises 2.322% by median in the repeat with
ranges overlapping; its initial full comparison rose 2.444% with separated
ranges. This remains an open regression concern.

Numeric-array stringify peak RSS rises **2.078125 MiB** in qualified pairs, and
both 20 MB parse peaks rise **0.3125 MiB**, all with separated ranges. Retaining
200,000 empty objects uses 24.65625 MiB current RSS versus the immediate parent's
24.96875 MiB. Earlier anchors have not been requalified in this experiment;
the previous 147-case matrix and its remaining regression requirements still
apply. No old requirement is cleared by an unpaired comparison.

## Validation and measurement scope

- 213 runtime JSON tests pass, including five new batch-construction tests.
- 42 full compiled Node comparisons pass: 40 core fixtures and two controls.
- 44 moving-GC runs pass: 42 full Node matches and two extended reference
  matches. Every stress run has actual collection and movement witnesses.
- The extended fixture still has the pre-existing inherited Object.prototype
  root-array `toJSON` mismatch. Earlier lazy/prototype gaps remain open; the
  separate pending `toJSON` changes are not part of this candidate.
- The full window includes 152 output checks, 456 timing trials and 432 memory
  trials, with 30 clean external observations. The qualified paired window
  includes 532 trials across 38 cases and 24 retained-empty trials, with 29 clean
  observations. Both windows pass entry/end quiet gates.
- The first paired window ended above the load limit. Its complete data is
  retained in `rejected-paired` and excluded from qualified conclusions.
- The root-holder gate passes. The file-size gate reports two unchanged parent
  violations; see `file-size-gate.log` and `validation.json`.

CPU is **amortized over the entire measured loop**, including GC before or after
individual JSON calls. It is not isolated API return latency. The worker retains
the most recent result in `last` for validation, so one output graph remains
live at loop safepoints. Default array-root parse may be lazy; stringify inputs
are eagerly materialized before timing. RSS is whole-process memory.

See [construction design](../../BATCH_BUILDER.md) and
[GC timing and deferral](gc-boundary.md). Collector policy and existing hook
implementations are unchanged. New arena allocation and page-finalization
helpers are explicit integration changes; the collector directory is not being
claimed globally unchanged from v0.5.1520 because the inherited diagnostic
forwarding checks remain present.

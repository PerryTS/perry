# Bounded JSON GC deferral

Measured source: `0327b9460a749592deb354d72ebad29bf4ae4bba`, on local branch
`codex/json-gc-deferral-1520`. Parent: the construction batch at `fb69a7059`
(runtime source `e9de6a96`). Both arms link the same pinned application object
against verified runtime/stdlib builds with matching features and build settings.
This scheduling change is separate from the earlier construction change and the
pending array `toJSON` work in the other worktree. Nothing has been published.

## Qualified results

Seven randomized pairs confirm the wide-object parse improvement:

| Metric | Construction parent | Deferred | Change |
|---|---:|---:|---:|
| CPU per parse, including caller GC | 20.452 ms | 15.191 ms | −25.73% |
| Whole-process peak RSS | 176.672 MiB | 167.438 MiB | −9.234 MiB |

Both observed ranges are separated. The wide fixture has one object with 50,000
properties. It remains slower than Node and Bun: in the full four-engine run,
Perry takes 15.180 ms, Node 4.920 ms, and Bun 4.166 ms. The experiment improves
Perry's own baseline; it does not establish parity on this workload.

The narrow admission removes the broad prototype's large parse regressions.
Seven-pair changes for 8 MiB and 20 MiB record-object parse are −0.77% and −0.72%,
with separated ranges. Empty, tiny, small-record and 1 KiB parse medians are
between −0.17% and −0.33%, with overlapping ranges.

Remaining regressions are included. Four stringify CPU increases have separated
paired ranges: 8 MiB record array **+0.77%**, 1 MiB record object **+0.83%**, tiny
object **+0.80%**, and wide object **+0.98%**. The largest separated paired peak-RSS
increase is **+0.547 MiB** for 8 MiB array-root parse. Several other peak increases
are 0.031–0.063 MiB; every row is retained in the tables. Retaining 200,000 empty
objects reads 24.734 MiB current RSS versus 24.656 MiB for the parent, while peak
RSS is 26.188 versus 26.219 MiB.

The four-engine target counts remain **16/38 CPU, 65/74 peak RSS and 33/36
retained RSS**. See [all 38 CPU rows](cpu-38.md), [all CPU/RSS targets](parity.md),
and [the 38 paired comparisons](paired/table.md). All-row parity, no regressions,
and complete semantic acceptance remain unachieved. Earlier regression anchors
and the older 147-case matrix have not been requalified by this experiment.

## What changed

JSON completion records pressure without running a collection before returning
the newly built graph. Entry checks can still service older debt. For eligible
document construction, the collector may defer ordinary nursery collection for
two precise safepoints or up to 8 MiB of additional arena high-water space,
whichever expires first. The allowance shrinks to 1/32 of a smaller heap budget.
Repeated parses share one allowance; a completed collection resets it.

Admission requires at least 256 KiB of input and measured managed construction
that fits the allowance, including allocation from reused holes. Tiny leaf
parses retain their old scheduling path. Large graphs receive no fresh grace
period. These are scheduling bounds, not a hard process-RSS cap: a suppressed
parse can overshoot the allowance by its construction size, and allocation
fallbacks retain their existing slack when no precise poll is reached.

GC suppression ends with construction. Headers, layouts and old-to-young edges
are complete before return; write barriers stay active. Old-generation pressure,
OS memory warnings, explicit collection and seed-selected stress collections
bypass the grace period. Existing incremental cycles continue normally. Native
tape memory remains counted as old-generation pressure. The new scheduling state
contains no managed pointers and adds no roots or per-object allocation list.

Stringify has no separate grace period and keeps its existing traversal/callback
behavior. Parse-then-stringify can use the parse's remaining allowance. See the
[design and limits](../../GC_DEFERRAL.md).

## Validation and measurement scope

- **3,278 runtime tests pass; four are ignored**, including nine new deferral
  tests. They exercise actual return-before-collection, expiry by bytes and
  polls, repeated-call non-renewal, small/large admission, retained-child motion,
  fewer than 100 copied objects after 999 sibling records die, and bypasses for
  seeded collection, explicit collection and pressure.
- The existing **42 compiled Node comparisons** pass. **44 existing moving-GC
  runs** pass: 42 Node matches and two extended reference matches, all with live
  collection/movement witnesses.
- A new document-sized lifetime fixture matches Node in **nine configurations**:
  default, 32 MiB heap, forced eager parsing, full-GC fallback, loop polls off,
  and four seeded moving configurations including small-heap/forced-eager runs.
- The inherited root-array `Object.prototype.toJSON` mismatch and earlier lazy
  property-operation gaps remain. This branch does not include their separate
  pending fixes.
- The root-holder, Node-version and GC-doc-claim checks pass. File-size and
  address-class gates fail on files that are byte-identical to the parent;
  see the logs and `validation.json`. No new root-holder inventory entry is
  needed because the new state contains only byte counts and flags.
- The full window has 152 verified outputs, 456 timing trials, 432 memory
  trials and 30 clean external observations. The paired window has 532 trials,
  24 additional retained-empty trials and 28 clean observations. Both pass
  the fixed entry/end load limit of 2.5 and foreign-workload checks.

The main worker measures the entire loop and keeps its newest output in `last`.
It does not report isolated API return latency. Array-root parse may be lazy;
stringify starts with eagerly materialized input. RSS includes the whole process.

The supplemental lifetime probe separates discard/latest/retain lifetimes, API
timing, loop CPU and final explicit cleanup CPU. Its extra clocks and cleanup
make it a separate workload. The final qualified run has 42 cases, 252 trials,
12 separate GC trace diagnostics and 29 clean external observations. Entry load
was 1.478 and ending load 2.269, within the fixed 2.5 limit.

| Supplemental parse workload | Total CPU including final cleanup | Peak RSS change |
|---|---:|---:|
| Wide object, discard output | −43.24% | −61.203 MiB |
| Wide object, keep latest | −43.86% | −45.703 MiB |
| Wide object, retain all eight | −0.11%, overlapping ranges | +0.094 MiB |
| 1 MiB record object, discard | −1.89% | +2.188 MiB |
| 1 MiB record object, keep latest | −3.70% | +2.562 MiB |
| 1 MiB record object, retain all 16 | −28.65% | −8.828 MiB |

The wide discard/latest gains persist after charging final cleanup, so this
workload does less total work rather than merely moving it outside the timed
region. Its API return durations themselves improve by less than 1%; most of
the reduction appears in caller collection work. Keeping all eight wide objects
shows no clear CPU gain. These are workload-specific outcomes, not a general
claim that retention prevents a gain: the retained 1 MiB record case benefits.

The supplemental regressions also remain open. Small-record stringify total CPU
increases **2.27–3.44%** across lifetimes, and 1 KiB retained parse increases
**1.12%**. The 20 MiB discard round trip increases **4.46%** including cleanup,
despite a 0.95% lower loop CPU: final cleanup rises from 3.515 to 70.199 ms.
Its peak RSS falls 21.062 MiB. The wide discard round trip saves 17.91% total CPU
but adds 20.844 MiB peak RSS. The 1 MiB round trip adds 0.77% CPU and 4.203 MiB
peak RSS. See [all 42 lifetime rows](lifetimes/table.md) and the complete metric
ranges in [their summary](lifetimes/summary.json). Three pairs and separated
observed ranges do not establish statistical confidence intervals.

The first final-source lifetime window ended above the load limit and is
excluded from qualified conclusions; its data is retained in
`lifetimes-unqualified`.
The earlier broad-admission experiment and its qualified lifetime results are
also preserved in `broad-admission`, without mixing their trials into this source.

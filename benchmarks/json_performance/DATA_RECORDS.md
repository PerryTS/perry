The latest measured source, shape-plans (`936c60675`), reduces paired heterogeneous stringify CPU **32.74%** versus nested-records and **69.17%** versus growth-alias. It uses a bounded native ShapeId table and one key-prefix pool while retaining per-instance safety checks. Peak RSS recovers the prior **0.453125 MiB** increase. Full-run heterogeneous stringify remains **1.47x Node**.

All-row inventory remains **12/38 CPU, 65/74 peak RSS and 33/36 retained RSS**. Full and 1,526-trial paired matrices qualify with 30/82 clean observations; 48 additional retained-empty trials and two profiles complete. No-regression acceptance remains false: numeric stringify, 1 KB object parse, small-record parse/stringify and older CPU anchors regress. Retained-empty RSS rises 0.109375 MiB versus the explicitly remeasured escaped-count anchor.

204 JSON runtime tests and 42 full compiled Node comparisons pass. One extended fixture retains the reference's inherited Object.prototype.toJSON mismatch; 44 moving-GC runs yield 42 full Node and two extended reference matches, with no new discrepancy. Earlier forced-lazy Object.* gaps remain open. All 77 source hashes are verified. GC policy/hooks and the inherited diagnostic boundary remain unchanged.

[Latest 38 CPU rows](results/shape-plans/cpu-38.md), [CPU and RAM](results/shape-plans/tables.md), [complete tradeoffs](results/shape-plans/README.md), [GC boundary](results/shape-plans/gc-coordination.md), [next work](results/shape-plans/next-steps.md).

Earlier checkpoints follow.

The latest measured source, nested-records (`27eeccf79e`), reduces paired heterogeneous stringify CPU **54.25%**, from **4.35 to 1.99 ms**, using bounded native per-call key prefixes and a callback-free traversal of ordinary nested records. It still takes **2.20x Node** in the full run, and peak RSS rises **0.453125 MiB** versus growth-alias.

All-row inventory remains **12/38 CPU, 65/74 peak RSS and 33/36 retained RSS**. Full and 994-trial paired matrices plus 24 retained-empty trials qualify with 31/54 clean observations. CPU and RSS regressions remain, including older anchors. All-row/no-regression acceptance is false.

202 JSON runtime tests and 42 full compiled Node comparisons pass. One extended fixture matches the reference while retaining a pre-existing inherited Object.prototype.toJSON root-array mismatch. Of 44 moving-GC runs, 42 match Node and two extended runs match the reference with that known mismatch; no candidate/reference regression is found. Earlier forced-lazy Object.* gaps remain open. All 77 source hashes are verified; GC policy and hooks are unchanged, with the inherited diagnostic-only repair explicitly documented.

[Latest 38 CPU rows](results/nested-records/cpu-38.md), [CPU and RAM](results/nested-records/tables.md), [complete tradeoffs](results/nested-records/README.md), [GC boundary](results/nested-records/gc-coordination.md), [next work](results/nested-records/next-steps.md).

Earlier checkpoints follow.

The latest measured source, growth-alias (`452c329aa`), fixes the verifier's rejection of explicitly retained old array-growth aliases while preserving strict evacuation and metadata checks. All 267 unique runtime tests, 42 compiled comparisons and 42 moving-GC runs pass, including the previously failing broad fixture and no-JSON control. Collection policy, thresholds and hooks are unchanged; GC production edits are diagnostic-only and explicitly documented.

All-row inventory remains **12/38 CPU, 65/74 peak RSS and 33/36 retained RSS**. Full and 966-trial paired matrices plus 24 retained-empty trials qualify with 31/53 clean observations. Against short-array, paired small-record stringify improves 1.51% and all peak-RSS medians fall 0.11-0.27 MiB; 1 KB object parse regresses 1.39% and wide stringify 0.88%. Older regressions and forced-lazy Object.* gaps remain open. All-row/no-regression acceptance is false.

[Latest 38 CPU rows](results/growth-alias/cpu-38.md), [CPU and RAM](results/growth-alias/tables.md), [complete tradeoffs](results/growth-alias/README.md), [GC boundary](results/growth-alias/gc-coordination.md), [next work](results/growth-alias/next-steps.md).

Earlier checkpoints follow.

The latest measured source, short-array (2e79d122), reduces paired 20 MB array/object-root stringify CPU **5.58%/5.37%** and peak RSS about **12.4 MiB**. Object-root parse peak falls **19.7/64.8/91.8 MiB** at 1/8/20 MB. Short parsed arrays now allocate only their completed width; lazy growth aliases and descriptor flags resolve the live array header.

All-row inventory is **12/38 CPU, 65/74 peak RSS and 33/36 retained RSS**. No-regression acceptance fails: null/string/empty/tiny parse and small-record stringify regress, most other peak rows rise slightly, and older numeric/heterogeneous/small-object regressions remain. Both new broad-fixture GC seeds fail; a no-JSON ordinary-array control fails identically on the reference and candidate. This correctness requirement remains open.

195 runtime tests, 40 default compiled comparisons, one forced-lazy comparison and 68 hashes pass. Moving-GC runs: 38 pass and two fail. Full and complete 896-trial paired matrices plus 24 retained-empty trials qualify with 31/51 clean observations. Four stronger canonical-key relocation tests now pass. Production GC policy and hooks remain unchanged.

[Latest 38 CPU rows](results/short-array/cpu-38.md), [CPU and RAM](results/short-array/tables.md), [complete tradeoffs](results/short-array/README.md), [open GC issue](results/short-array/gc-open-issue.md), [GC boundary](results/short-array/gc-coordination.md), [next work](results/short-array/next-steps.md).

Earlier checkpoints follow.

The latest measured source, inline-object (9187bd95), reduces paired tiny-object
parse CPU **67.04%**, from 0.363078 to 0.1196715 microseconds (3.03x faster).
It decodes eligible flat objects into inline native storage before collection,
then allocates only their fresh result with the ordinary allocator. GC policy
and hook implementations remain unchanged; JSON call sequences are documented.

All-row acceptance remains **12/38 CPU, 58/74 peak RSS and 28/36 retained RSS**.
No-regression acceptance fails: escaped stringify +1.41%, 16 KB array parse
+0.66% and 1 MB array parse +0.39% versus marker-probe are confirmed. Older
small/object-root parse and memory regressions remain. Full tiny parse is still
2.64x Bun. Most peak RSS rows recover roughly 0.1-0.2 MiB versus marker-probe.

182 runtime tests, 39 compiled comparisons, 36 moving-GC runs and 63 hashes pass.
Complete full and 854-trial paired matrices plus 24 retained-empty trials qualify
with 31/50 clean observations. A stronger direct keys-array relocation test remains
follow-up validation and is not claimed as completed.

[Latest 38 CPU rows](results/inline-object/cpu-38.md),
[CPU and RAM](results/inline-object/tables.md),
[complete tradeoffs and validation](results/inline-object/README.md),
[GC boundary](results/inline-object/gc-coordination.md), and
[next work](results/inline-object/next-steps.md).

Earlier checkpoints follow.

The latest measured source, marker-probe (7be2932fa), reduces paired stringify CPU
3.10% for tiny objects, 4.36% for 16 KB record arrays, and 2.36-2.78% for
1-8 MB records versus plan-scan. It also introduces confirmed parse regressions:
tiny/small/1 KB objects +3.21%/+2.42%/+2.31%, and 1/8 MB object-root records
+0.72%/+0.34%. Most whole-process peak RSS rows rise; the older 8 MB array-parse
peak penalty is 11.46875 MiB versus parse-entry. No-regression acceptance fails.

All-row acceptance remains **12/38 CPU, 58/74 peak RSS and 28/36 retained RSS**.
176 runtime tests, 38 compiled comparisons, 34 moving-GC runs and 60 hashes pass.
Complete full and 784-trial paired matrices plus 24 retained-empty trials qualify
with 32/46 clean observations; the first failed admission is preserved separately.
GC production policy and hook implementations are unchanged.

[Latest 38 CPU rows](results/marker-probe/cpu-38.md),
[CPU and RAM](results/marker-probe/tables.md),
[complete tradeoffs and validation](results/marker-probe/README.md), and
[next work](results/marker-probe/next-steps.md).

Earlier checkpoints follow.

The latest measured source, plan-scan (7117f9e67), confines short-word scans to
scalar output planning and restores the shared scanner. Paired small-record
stringify CPU improves **10.47%**, from 0.320849 to 0.287247 microseconds, with
peak RSS down 0.078125 MiB. Empty/tiny/small/1 KB object parse improve
3.80%/2.20%/1.45%/1.48%; all four also improve versus empty-parse, recovering the
earlier measured regressions. Perry still uses 2.61x Node's CPU on small-record
stringify in the full run.

**All-row and no-regression acceptance remain open: 12/38 CPU, 58/74 peak RSS
and 28/36 retained RSS targets.** Confirmed paired regressions versus
escaped-count remain: escaped stringify +1.55%, wide stringify +0.99%, and
20 MB object-root parse +0.48%. Numeric parse is +1.12% with overlapping ranges
versus escaped-count, and +1.60% with separated ranges versus tape-owned.
Inline-string stringify peak RSS rises 0.015625 MiB, retained-empty RSS rises
0.09375 MiB, and the older 8 MB array-parse peak regression remains
+11.265625 MiB versus parse-entry. All other unresolved regressions remain.

172 runtime tests, 38 pinned compiled comparisons, 34 moving-GC runs and all
59 source hashes pass. Matched release settings and complete full 152/456/432
and paired 756-trial matrices plus 24 retained-empty trials qualify with 32/44
clean observations. A separate quiet M1 20 MB profile finds 1,489/2,210 main-thread
samples under the loop GC safepoint, mostly full mark/sweep. This is sampling
evidence, not a measured GC CPU percentage; GC production policy is unchanged.

[Latest 38 CPU rows](results/plan-scan/cpu-38.md),
[CPU and RAM](results/plan-scan/tables.md),
[all target comparisons](results/plan-scan/parity.md),
[paired ranges](results/plan-scan/recheck-plan-scan/summary.json),
[validation and limitations](results/plan-scan/README.md),
[20 MB profile](results/plan-scan/large-record-profile/README.md), and
[next work](results/plan-scan/next-steps.md).

The shared-tail experiment below is retained as evidence of its tradeoffs.

The preceding experiment, short-tail (090e120f3), reduces paired
small-record stringify CPU 9.82%, from 0.3212 to 0.2897 microseconds. It also
introduces confirmed regressions, including long-ASCII parse +3.69%, 1 KB object
parse +1.51%, escaped stringify +1.36%, wide stringify +0.87%, and numeric parse
+0.69%. Scalar peak RSS rises by 0.03125–0.078125 MiB and retained-empty RSS by
0.203125 MiB. No-regression acceptance fails; escaped-count remains the retained
comparison reference. The next experiment limits the new scan to scalar output
planning to test whether the small-record benefit survives that restriction.

172 runtime tests, 38 pinned compiled Node comparisons, 34 moving-GC runs and
59 source hashes pass. Full 152/456/432 and paired 700-trial matrices plus 24
retained-empty trials qualify with 32/42 clean observations. All-row acceptance
remains open at **12/38 CPU, 58/74 peak RSS and 28/36 retained RSS**. All older
unresolved regressions remain requirements.
[All 38 experimental CPU rows](results/short-tail/cpu-38.md),
[CPU and RAM](results/short-tail/tables.md),
[paired ranges](results/short-tail/recheck-short-tail/summary.json),
[validation and limitations](results/short-tail/README.md), and
[next comparison](results/short-tail/next-steps.md).

The retained escaped-count checkpoint follows.

This source writes validated escaped UTF-8 strings directly into final
output, using bounded vector blocks to count expansion on ARM and a scalar
lookup table elsewhere. Native plans contain only counts; input pointers are
rederived after final allocation. Malformed UTF-8/WTF-8 retains the general
writer. GC production policy, thresholds and parse-boundary hooks are unchanged.

**All-row and no-regression acceptance remain open: 12/38 CPU, 58/74 peak RSS
and 28/36 retained RSS medians meet the better Node/Bun median.** Measured
source 4b9d577d4e81518f83a8b4a1e4f319d328597f74 passes 172 JSON runtime tests,
38 compiled Node comparisons and 34 moving-GC runs; all 59 source hashes and
matched build settings are verified. The full 152-verification / 456-timing /
432-memory matrix and 644-trial paired matrix plus 24 retained-empty trials
pass their quiet gates with 32/38 clean observations.

Paired escaped stringify CPU falls 49.97% versus empty-parse, from 1.8997 to
0.9504 ms, with peak RSS down 0.84375 MiB. In the full run Perry is about 2x
faster than Node and 2.2x faster than Bun on this row. Vector sizing improves
38.93% versus the first direct-output worker, while peak RSS rises 0.09375 MiB
at the same call count. Wide/16 KB record-array/heterogeneous stringify improve
1.99%/1.74%/1.01% versus empty-parse, with separated paired ranges.

Confirmed regressions versus empty-parse: small-record stringify +1.60%,
empty/tiny/small/1 KB object parse +2.44%/+0.95%/+0.56%/+0.58%, and tiny-object
stringify +0.89%. Eight-MB array stringify peak RSS rises 0.046875 MiB, and the
older eight-MB array-parse peak regression remains +11.359375 MiB versus
parse-entry. Other earlier unresolved regressions remain requirements.
Retaining 200k empty objects uses 24.64 MiB versus reference 24.86, Node 83.44
and Bun 50.20 MiB. Extra retained-empty trials do not change target counts.

[Latest 38 CPU rows](results/escaped-count/cpu-38.md),
[CPU and RAM](results/escaped-count/tables.md),
[every target](results/escaped-count/parity.md),
[paired ranges](results/escaped-count/recheck-escaped-count/summary.json),
[implementation and validation](results/escaped-count/README.md), and
[next regression recovery](results/escaped-count/next-steps.md).
The first direct-output experiment and its complete paired evidence are
preserved in [escaped-output](results/escaped-output/README.md).
Array-root parsing may defer materialization; stringify starts fully materialized.

The compact-plan experiment (8d587c863) is rejected and reverted in
692a8c94f. Although generated emitters shrink, its qualified full and 700-trial
paired matrices show small-record stringify +3.62%, escaped stringify +1.56%,
and record-array parse +1.26–1.55% versus escaped-count, with separated ranges.
All 59 source hashes were restored before the next experiment. Initial disk
exhaustion affected 16 fixture links; unchanged retries complete all 38 compiled
comparisons, alongside 172 runtime tests and 34 moving-GC runs.
[Complete rejected experiment](results/compact-piece/README.md).

The Piece-borrowing experiment (96cda0524) is rejected and reverted in
7eb11ae1d. Its qualified full and 616-trial paired matrices show 4.99-5.53%
slower record-array parsing and 1.27% slower small-record stringify, with
separated paired ranges. Smaller stringify gains do not justify these
regressions. All 56 restored source hashes match the empty-parse checkpoint
below. [Complete rejected experiment](results/piece-ref/README.md).

The latest measured source parses complete empty ordinary objects with only
its final allocation. It validates all input bytes before GC, preserves fresh
identity, ordinary-object behavior, pending debt and pressure scheduling, and
removes the leaf's managed scratch and suppression/rebaseline cycle. GC
production policy and thresholds remain unchanged.

**All-row parity and no-regression acceptance remain open: 11/38 CPU,
58/74 peak RSS and 28/36 retained RSS medians meet the better Node/Bun median.**
Measured source 1ebaf1f40eb834dea148461636111acdec14da24 passes 165 JSON runtime
tests, 37 compiled Node comparisons and 32 moving-GC runs; all 56 source hashes
match. The full 152-verification / 456-timing / 432-memory matrix passes with
32 clean observations. A 574-trial paired run plus 24 extra retained-empty
trials passes its quiet gate with 35 clean observations.

Paired empty-object parse CPU falls 82.08%, from 0.3224685 to 0.0577755
microseconds (5.58x faster). Bun remains about 2.4x faster in the full matrix.
Inline-string parse improves 7.18%, recovering the preceding tape-owned
regression, and 1 MB object-root parse improves 0.78%, with separated ranges.

New confirmed regressions against tape-owned: escaped stringify +2.22%, wide
stringify +2.14%, and numeric parse +1.52%. Empty-parse peak RSS rises 0.375 MiB;
several other whole-process RSS medians rise roughly 0.17-0.23 MiB. Retaining
200,000 empty objects uses 24.86 MiB versus Node 83.42 / Bun 50.19 MiB, but also
0.17 MiB more than the preceding Perry worker. Older unresolved CPU and memory
regressions remain requirements. The original target inventory excludes these
extra retained-empty trials.

[Latest 38 CPU rows](results/empty-parse/cpu-38.md),
[CPU and RAM](results/empty-parse/tables.md),
[every target](results/empty-parse/parity.md),
[paired CPU/RSS ranges](results/empty-parse/recheck-empty-parse/summary.json),
[retained-empty results](results/empty-parse/retained-empty/summary.json), and
[implementation and validation](results/empty-parse/README.md).
Array-root parsing may defer materialization; stringify starts fully materialized.

The preceding tape-owned source transfers large completed native tapes into their
lazy result's exact-size side storage. Small tapes retain reusable scratch.
A lazy-array assignment fix roots receiver/key/value through materialization
and preserves writes and the original alias's length. GC production policy,
thresholds and finalization remain unchanged.

**All-row parity and no-regression acceptance remain open: 12/38 CPU,
58/74 peak RSS and 28/36 retained RSS targets meet the better Node/Bun median.**
Measured source cc1c4f582c29c5bc12e54bf15f6f81a1226ad857 passes 161 JSON runtime
and five dynamic-index tests, 36 compiled Node comparisons and 30 moving-GC
runs. All 54 source hashes match. The full 152-verification / 456-timing /
432-memory matrix passes with 32 clean observations. A 574-trial paired repeat
passes with 35 clean observations; the first run failed its ending load gate
and is preserved as unqualified.

Paired 8 MB array-parse peak RSS falls 3.28 MiB against integer-piece, from
102.64 to 99.36 MiB, but remains 11.20 MiB above the older parse-entry anchor.
Its CPU falls 1.81%; long-ASCII/Unicode parse improves 5.12%/2.76%, 1 KB object
parse 2.93%, and numeric stringify 2.50%, with separated ranges.

Confirmed CPU regressions against integer-piece include inline-string parse
+5.71%, 1 MB record-array parse +2.38%, numeric parse +0.86%, and heterogeneous
stringify +0.69%. Heterogeneous/numeric parse peak RSS rises 1.22/0.33 MiB.
All prior unresolved regressions remain requirements. The 12th CPU median win,
escaped stringify by 0.3%, is effectively a tie rather than a proven lead.

[Latest 38 CPU rows](results/tape-owned/cpu-38.md),
[CPU and RAM](results/tape-owned/tables.md),
[every target](results/tape-owned/parity.md),
[paired CPU/RSS ranges](results/tape-owned/recheck-tape-owned/summary.json), and
[implementation and validation](results/tape-owned/README.md).
Array-root parsing may defer materialization; stringify starts fully materialized.

The preceding integer-piece source formats exact integer-valued f64 fields directly
in output plans, preserving signed-zero and shortest-roundtrip semantics.
It builds on direct final output for bounded records and the corrected parse
input lifetime and array-getter handling. GC production policy is unchanged.

**All-row parity and no-regression acceptance remain open: 11/38 CPU,
58/74 peak RSS and 28/36 retained RSS targets are met.** Measured source
64b5df4b008f746b88b0c99c99ee784fe241df09 passes 152 JSON runtime tests,
35 compiled Node comparisons and 28 moving-GC runs; all 49 source hashes match.
Its full 152-verification / 456-timing / 432-memory matrix passes with 32 clean
observations. A 532-trial paired run covers every CPU row with 34 clean
observations. Both quiet gates pass.

Against record-output, paired tiny-object stringify falls 30.20%
(0.13656 to 0.09533 microseconds), small-record stringify 21.59%
(0.40287 to 0.31587), and 1 KB object stringify 14.40% (0.28632 to 0.24509).
Inline-string stringify improves 3.12%, 16 KB record-array stringify 1.76%,
and heterogeneous parse 2.26%, with separated observed ranges.

The paired run also confirms regressions: empty/tiny/small/1 KB object parse
+3.39%/+2.87%/+1.79%/+4.53%; null/inline-string parse +2.33%/+1.93%; long-ASCII/
Unicode parse +5.41%/+3.35%; and smaller large-record parse and heterogeneous
stringify increases. Peak RSS rises by roughly 0.03–0.14 MiB in several rows,
and the preceding 8 MB lazy-parse memory regression remains. The parse source
is unchanged in this candidate. Equal parser instruction counts/mnemonics and
matching local GC counts/final arena sizes do not establish equal execution
cost or explain away the qualified regressions.

[Latest 38 CPU rows and RAM](results/integer-piece/tables.md),
[every target](results/integer-piece/parity.md),
[paired CPU/RSS ranges](results/integer-piece/recheck-integer-piece/summary.json),
[implementation and validation](results/integer-piece/README.md), and
[next tape-ownership experiment](results/integer-piece/next-steps.md).
Array-root parsing may defer materialization; stringify starts fully materialized.
The preceding checkpoints below retain their complete gains and regressions.

The preceding record-output source writes bounded ordinary records
with primitive-array fields directly into the final string. Its stack plan
holds lengths, indices and inline scalar bytes; the parent root traces its
children, which are rederived after allocation. An array-getter correctness
fix routes descriptor/prototype-bearing arrays through ordered Get operations.
The tape scanner builds before collection through a raw-input API whose byte
borrow ends before collection. GC production policy remains unchanged.

**All-row parity and no-regression acceptance remain open: 11/38 CPU,
58/74 peak RSS and 28/36 retained RSS targets are met.** Measured source
53052fc09d47d956e8decf22f677a773f94d42cb passes 151 JSON runtime tests,
35 compiled Node comparisons and 28 moving-GC runs. All 49 source hashes match.
The full matrix passes 152 verifications, 456 timing trials and 432 memory
trials with 33 clean external observations. Its 686-trial paired run covers
49 cases with 45 clean observations. Both windows pass their quiet gates.

Paired small-record stringify improves 23.14% against parse-entry, from about
0.524 to 0.403 microseconds. Tiny stringify improves 2.73%; empty/tiny parsing
improves 1.76%/0.79%. The earlier 1 MB record-array and heterogeneous parse peak
RSS regressions recover by 4.86/3.88 MiB. However, 8 MB record-array parse peak
RSS rises 14.31 MiB, to 102.48 MiB. CPU regressions include inline-string
stringify +3.22%, numeric stringify +2.19%, wide stringify +1.12%, 16 KB/1 MB/8 MB
array parse +1.68%/+1.41%/+1.75%, and heterogeneous parse +3.50%, with separated
observed ranges. Additional smaller differences remain in the complete data.

[Latest 38 CPU rows](results/record-output/cpu-38.md),
[CPU and RAM](results/record-output/tables.md),
[every target](results/record-output/parity.md),
[paired CPU/RSS ranges](results/record-output/recheck-record-output/summary.json),
and [implementation and validation](results/record-output/README.md).
Local instruction counts fall 27.57% for small-record stringify. Sampling
identifies Ryū formatting of integer-valued fields as a remaining cost; the
next candidate addresses that cost and is not included in these measurements.
Array-root parsing may defer materialization; stringify starts fully materialized.

The preceding parse-entry source fixes parse-input rooting before pending collection,
separates scalar decoding from the container entry, and restores direct
primitive emission for nested one-field objects. **All-row parity and
no-regression acceptance remain open: 11/38 CPU, 58/74 peak RSS and 28/36
retained RSS targets are met.**

The new forced-collection tests reproduce valid-input SyntaxErrors on the
preceding ordinary direct/lazy entry; the fallible entry is a passing control.
The input now stays rooted before collection and is re-read afterward. All
144 runtime tests, 34 compiled Node comparisons and 26 moving-GC runs pass.
GC production policy and thresholds are unchanged. Tape input borrowing and
suppression ordering change locally to keep references out of collection.
All 44 recorded source hashes match a7fda79684c60f79311e4c19d5aabf0436f7772e.

The full matrix has 152 verifications, 456 timing trials and 432 memory trials
with 32 clean observations. The paired retry has 616 trials across 44 cases
with 42 clean observations; a load rejection before its first admission is
preserved. Both completed windows pass their load and process gates.

Paired heterogeneous stringify uses 4.85% less CPU than bounded-preflight and
3.37% less than primitive-object, recovering its recent regression. Tiny-object
parse improves 1.12%, small-record parse 0.95%, and 16 KB record-array stringify
1.56%. Large record parsing recovers about 0.8–1.0% versus bounded-preflight,
but 8/20 MB cases remain 1.26–1.63% slower than primitive-object.

New failures remain explicit: null/inline-string parsing is 2.31%/2.05% slower,
escaped stringify 2.52% slower, and 1 MB record-array parse 0.23% slower, with
separated paired ranges. Peak RSS rises 4.73 MiB for 1 MB record-array parse
and 3.77 MiB for heterogeneous parse. Their final arena sizes and old-reclaim
scan counts are essentially unchanged in local diagnostics, which do not
explain transient/system allocator memory or prove equal GC time.

[Current 38 CPU rows and RAM](results/parse-entry/tables.md),
[every target](results/parse-entry/parity.md),
[paired CPU/RSS ranges](results/parse-entry/recheck-parse-entry/summary.json),
and [implementation, reproducer and validation](results/parse-entry/README.md).
The preceding checkpoints below remain historical evidence.

The preceding bounded-preflight checkpoint adds scalar parsing, guarded record emission,
bounded depth preflight and direct emission of wide primitive objects. **This
is a development checkpoint. CPU/RSS parity and no-regression acceptance remain
open: 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS targets are met.**

Depth preflight skips inputs too short to exceed the nesting limit; the parser
still validates syntax. Wide primitive-field preflight borrows inline storage
once before emission. Small objects keep the prior closure scan. These changes
pass 139 runtime tests, 33 compiled Node comparisons and 24 moving-GC runs.
All 43 recorded source hashes match 3ffe5690410f7a8ea110c3f9e356ebb8eb7933b1.

The full matrix has 152 output verifications, 456 timing trials and 432 memory
trials with 31 clean observations. A separate 546-trial paired check covers
39 cases with 39 clean observations. Both windows pass load and process gates.
Paired small-record parsing uses 9.99% less CPU, tiny-object parsing 4.57% less,
empty-object parsing 2.17% less, and wide-object stringify 7.15% less than the
preceding primitive-object release. Against the older empty-object release,
small-record parse improves 9.59%, tiny-object parse 2.46%, and wide-object
stringify 31.69%. Wide stringify is now about 1.69 ms with peak RSS near 71 MiB.

The no-regression requirement still fails: paired 1/8/20 MB record-object
parsing is 1.78%/2.13%/2.32% slower than primitive-object; 20 MB record-array
parsing is 2.80% slower; heterogeneous stringify is 1.60% slower. Small-record
stringify is unchanged against primitive-object but still 1.68% slower than
empty-object, and inline-string parse is still 2.03% slower than empty-object.
All other rows and ranges, including smaller differences, remain in the data.

Local sampling confirms small-record stringify enters shape-template emission
before the changed generic preflight. Per-call template construction and
allocation are material costs, weakening the earlier speculative-preflight
attribution. A bounded direct-output path for primitive-array records and
outlining the large scalar frame away from container parsing are the next
investigations. The current preflight changes preserve GC scheduling and policy.

[Current 38 CPU rows and RAM](results/bounded-preflight/tables.md),
[every target](results/bounded-preflight/parity.md),
[paired ranges](results/bounded-preflight/recheck-bounded-preflight/summary.json),
and [validation and diagnostics](results/bounded-preflight/README.md).
The preceding checkpoints below remain as historical evidence.

The primitive-object walk reuses the generic input root, prototype handling,
field preflight and key order, then borrows inline keys and values once. Its
successful interval cannot invoke callbacks, allocate managed scratch or collect.
Descriptors, class instances, overflow and complex fields retain the general walk.
138 runtime tests, 33 compiled Node comparisons and 24 moving-GC runs pass.
All 43 recorded source hashes match 1196a5f84c2976d126bd193839ed00588643234a.

The full matrix contains 152 verifications, 456 timing trials and 432 memory
trials, with 33 clean external observations. A separate 350-trial paired check
covers 25 cases with 25 clean observations. Both windows pass their load and
competing-process gates. Paired wide-object stringify CPU falls 26.28% versus
the empty-object release, from 2.475 ms to 1.825 ms, with about 71 MiB peak RSS.
The full matrix has Node at 6.368 ms and Bun at 0.665 ms: Perry still needs
2.75 times Bun's CPU on this case.

Paired tiny-object parse regresses 2.21%, small-record stringify 2.15%, null
parse 4.80% (about 0.63 ns), inline-string parse 3.96%, small-record parse 0.73%,
wide-object parse 0.98%, and empty-object parse 0.22%. Escaped stringify's
apparent full-matrix regression does not reproduce. Numeric parse improves
1.36% against the immediate reference; its older Unicode regression remains
open. Reducing speculative primitive preflight on small records and investigating
container/scalar wrapper costs are the next steps. These rows are not accepted
as regression-free merely because the wide-object result improved.

[Primitive-object 38 CPU rows and RAM](results/primitive-object/tables.md),
[every target](results/primitive-object/parity.md),
[paired ranges](results/primitive-object/recheck-primitive-object/summary.json),
and [implementation and validation](results/primitive-object/README.md).
The earlier checkpoints below remain as historical evidence.

A subsequent change reuses the record preflight when emitting primitive-array
children. It passes 133 runtime tests, 31 compiled Node comparisons and 20
moving-GC stress runs, and removes about 5.5% of local process instructions on
16 KB/1 MB records. Its first benchmark admission was rejected before taking
measurements because a foreign worker had started. Its second full run was
unqualified because foreign workers started midway. Its implementation is
included in the current qualified empty-object release below.
[Validation-reuse implementation and evidence](results/record-array-proof/README.md).

The preceding empty-object follow-up returns its inline output before field
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
[Empty-object 38 CPU rows and RAM](results/empty-object-leaf/tables.md),
[every target](results/empty-object-leaf/parity.md), and
[paired ranges](results/empty-object-leaf/recheck-empty-object-leaf/summary.json).
That measured source is f7bc8848770d5b03422478de6e44384feeae6015;
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

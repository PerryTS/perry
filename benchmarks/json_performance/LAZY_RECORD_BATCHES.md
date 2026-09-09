# Lazy record construction: candidate measurements

These candidates are **not accepted or merged**. The reference is the exact
PR #10022 merge, `c82e613d260a80324914b5aba503a31a763ad90e`. Each candidate
relinks the same benchmark object against its newly built runtime. Both workers
run in fresh processes with equal iteration counts, interleaved with Node
26.5.1 and Bun 1.3.14 on the dedicated M1/8 GiB host.

## Producer change

An existing lazy array supplies the byte range of one requested record. For
records of at most 512 source bytes, the candidate uses the direct parser's
ordinary construction batch. The input remains rooted; collection is suppressed
through construction; the completed result is rooted before suppression ends.
Larger records retain the source-backed tape walker. Whole-array reparsing also
uses the batch constructor instead of the ordinary constructor.

This borrows the nursery cursor. It does not reserve a separate slab or heap.
Completed objects keep ordinary GC lifetimes, including tracing after return.

## First scheduling candidate

The first version checked GC before every bounded record. Full scans improved
25–75%, but the 7.1 MiB sparse row used 5.3% more CPU, consistently across all
five samples. Its lower RSS did not waive that regression. The 13 KiB sparse
row also increased peak RSS by 1.5 MiB.

[Complete R1 comparison](results/quiet-lazy-batch-ab-r5/parity.md).

## Deferred scheduling candidate (R2)

R2 services existing pending parse debt before construction and samples pressure
every 64 bounded completions. Full-GC mode and active budgeted cycles retain
the ordinary trigger check. No new GC environment knob is introduced.

| Operation | Reference CPU µs | R2 CPU µs | Change | R2 / fastest competitor |
|---|---:|---:|---:|---:|
| 13 KiB full scan | 289.530 | 59.760 | −79.4% | 1.674× |
| 0.9 MiB full scan | 4,620.103 | 3,254.574 | −29.6% | 1.462× |
| 7.1 MiB full scan | 36,471.875 | 24,977.250 | −31.5% | 1.169× |
| 7.1 MiB sparse read | 11,274.167 | 11,238.917 | −0.3% | 0.526× |
| 17.8 MiB roundtrip | 98,893.000 | 98,683.000 | −0.2% | 1.299× |

The sparse CPU regression disappeared. However, the 13 KiB sparse peak still
rose from 62.484 to 64.328 MiB. Two tiny stringify medians also increased:
`string_a` by 3.2% and `small_record` by 4.8% (about 0.3 and 2.2 ns/call).
Those CPU differences need focused replication before attribution; there is
no stringify source change in this candidate. The reproducible RSS increase
alone is sufficient to reject R2 under the no-regression requirement.

The full inventory remains 46/50 CPU targets, 67/86 peak-RSS targets, and 31/36
post-operation RSS targets at or below the better competitor median. These
counts do not include rotating-input coverage. R2 was rejected before running
that additional acceptance suite.

Validation completed:

- 275 JSON runtime tests and three census tests passed with one test thread.
- Matched compiler/runtime/stdlib release build passed.
- 200/200 original-worker correctness checks passed; all 344 measurement groups
  contain five samples and complete engine coverage.
- Scheduled moving-GC scan matched Node, with 1,323 copying minors, 207,699
  moved objects, and 1,323 protected retired page sets. Full-GC output also
  matched Node.
- Required source-size, address-classification, formatting, and runtime-root
  audits passed. The census pin was re-audited for the read-only policy query's
  visibility change; the synchronous census window did not change.
- The dedicated host passed quiet admission before and after measurement.

[All R2 CPU/RSS rows](results/quiet-lazy-batch-r2-ab-r5/parity.md),
[raw timings](results/quiet-lazy-batch-r2-ab-r5/timing.jsonl),
[retained-output measurements](results/quiet-lazy-batch-r2-ab-r5/memory.jsonl),
[quiet window](results/quiet-lazy-batch-r2-ab-r5/window.json),
[binary/source provenance](results/quiet-lazy-batch-r2-ab-r5/provenance.json), and
[candidate source patch](results/quiet-lazy-batch-r2-ab-r5/source.patch).

## Traversal dispatch candidate (R3)

R3 requires consecutive cold reads before selecting the batch producer.
Isolated reads keep the existing tape producer and avoid introducing another
parser's shared shape metadata. The existing adaptive whole-array fallback
remains in place.

The full quiet-host comparison passed 200 correctness checks and all 344
measurement groups. Scan CPU improved 78.1%, 29.8%, and 31.5% on the 13 KiB,
0.9 MiB, and 7.1 MiB fixtures. The sparse RSS increase disappeared: the 13 KiB
row measured 57.45 versus 57.50 MiB for its interleaved reference. No peak or
post-operation RSS increase exceeded 0.75 MiB in this run.

R3 remains unaccepted. The `string_a` and `small_record` stringify CPU increases
repeated at 3.1% and 4.9%. The Unicode stringify median was also 9.9% higher,
with broadly overlapping samples (candidate 26.34–33.04 µs, reference
26.12–30.13 µs); that row needs a longer comparison before attribution.
Initial disassembly inspection found code/data placement differences. Their
effect on the tiny rows still needs a controlled comparison; it is not an
established cause.

[All R3 CPU/RSS rows](results/quiet-lazy-batch-r3-ab-r5/parity.md) and
[exact source patch](results/quiet-lazy-batch-r3-ab-r5/source.patch).

The next candidate also addresses the independently discovered
[large-string output-debt defect](LARGE_STRING_RECLAMATION.md). It still requires
all original and rotating-input CPU/RSS checks before acceptance.

Reproduce a complete original-run summary with
`python3 summarize_original.py RESULTS --engines perry,node,bun,baseline`, then
`python3 parity.py RESULTS/summary.json --output RESULTS`. The summary checker
rejects missing correctness cells, unequal timing work, incomplete retained
groups, missing or duplicate repetitions, and failed quiet admission.

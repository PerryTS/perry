# MongoDB async iterator lifetime — #11726 / PR #11731

On Linux x86_64, the real npm MongoDB 7.6.0 driver is compiled natively and run against a private `mongod`, with matching Node stdout/checksums and no removed binding symbols. This harness uses a real server; the separate MongoDB gap fixture uses the in-process fake server.

| Metric | Base `437520d02b` | Final `ca77316359` | Node in final run |
| --- | ---: | ---: | ---: |
| Instructions / operation | 207,720,147 | 37,352,625 | 867,844 |
| Peak RSS (KiB) | 1,028,816 | 210,400 | 118,476 |

RSS is **1.78× Node**, below the issue's 2× target. Instructions fall **82.0%**; RSS falls **79.5%**. The final frame-pointer two-N profile attributes **4.04%** of instructions to all `gc_*` buckets (minor 2.73%, major 0.01%, other GC 1.30%) and reaches `main` on 99.9% of samples. Earlier core-fix DWARF and frame-pointer profiles measured 4.65% and 4.68% respectively, with 95.8% and 99.9% unwind coverage.

The changes close nested `for await` iterators when their containing generator is externally returned, finish enclosing finalizers after awaited cleanup, and remove the permanent runtime root of idle generators' original throw closures. The latter root retained captured buffers and queues; missing iterator close retained per-command data listeners. Both effects made later responses and GC increasingly expensive.

## Method and limits

- Node 26.5.1; release compiler and runtime/static wrappers built together, automatic runtime specialization disabled. Baseline and candidate archives are isolated. No version bump.
- `scripts/package_bench.py run --filter mongodb/insert_find --arms node,perry --modes instr,rss`, with three instruction repetitions, `n1=500`, `n2=2000`, `warm=200`.
- `scripts/package_bench.py profile --callgraph --exact --filter mongodb/insert_find` uses the same two-N subtraction on `instructions:u`. The DWARF profile uses the harness defaults. The frame-pointer cross-check passes `--callgraph-mode fp`; its `perf record` invocation additionally uses `-B --no-buildid-cache` to avoid expensive post-record debug-symbol processing.
- These are shared-host, load-flagged runs. No wall-time claim is made. Per-iteration counters, sampled profiles and peak RSS are retained in the adjacent JSON files.
- The 51-row package comparison is complete: 50 rows pass Node correctness in both arms; `fastify/inject` has an identical pre-existing failure. Repeated noisy readings show no sustained material regression. Matrix iteration counts are scaled to 0.1× with warm-up unchanged, except full-size insert/find; see [package-comparison.md](package-comparison.md) for exact revision provenance, counters and pre-existing limitations.

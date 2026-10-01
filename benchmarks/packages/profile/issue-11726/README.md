# MongoDB async iterator lifetime — #11726 / PR #11731

On Linux x86_64, the real npm MongoDB 7.6.0 driver is compiled natively and run against a private `mongod`, with matching Node stdout/checksums and no removed binding symbols. This harness uses a real server; the separate MongoDB gap fixture uses the in-process fake server.

| Metric | Base `437520d02b` | Candidate | Node in candidate run |
| --- | ---: | ---: | ---: |
| Instructions / operation | 207,720,147 | 37,362,524 | 798,835 |
| Peak RSS (KiB) | 1,028,816 | 211,684 | 119,012 |

RSS is **1.78× Node**, below the issue's 2× target. Instructions fall **82.0%**; RSS falls **79.4%**. The two-N instruction profile attributes **4.65%** of instructions to all `gc_*` buckets (minor 2.87%, major 0.14%, other GC 1.63%). A frame-pointer cross-check gives **4.68%**. DWARF and frame-pointer profiles reach `main` on 95.8% and 99.9% of samples respectively.

The changes close nested `for await` iterators when their containing generator is externally returned, finish enclosing finalizers after awaited cleanup, and remove the permanent runtime root of idle generators' original throw closures. The latter root retained captured buffers and queues; missing iterator close retained per-command data listeners. Both effects made later responses and GC increasingly expensive.

## Method and limits

- Node 26.5.1; release compiler and runtime/static wrappers built together, automatic runtime specialization disabled. Baseline and candidate archives are isolated. No version bump.
- `scripts/package_bench.py run --filter mongodb/insert_find --arms node,perry --modes instr,rss`, with three instruction repetitions, `n1=500`, `n2=2000`, `warm=200`.
- `scripts/package_bench.py profile --callgraph --exact --filter mongodb/insert_find` uses the same two-N subtraction on `instructions:u`. The DWARF profile uses the harness defaults. The frame-pointer cross-check passes `--callgraph-mode fp`; its `perf record` invocation additionally uses `-B --no-buildid-cache` to avoid expensive post-record debug-symbol processing.
- These are shared-host, load-flagged runs. No wall-time claim is made. Per-iteration counters, sampled profiles and peak RSS are retained in the adjacent JSON files.
- Other package matrix validation is still running and will be added before marking the PR ready.

# Bound retained JSON parse-shape metadata

Follow-up: [Unicode validation and counting](UNICODE.md) measures a further
candidate. This document preserves the cache-only results.

The parse-shape cache now retains at most **4,096 total key slots**, in
addition to its existing 256-entry limit. Shapes wider than that bypass the
cache. Smaller shapes still reuse matching entries; an entry that would exceed
the total key budget is returned as part of the parsed graph without being
retained by the parser cache. Array allocation, copy-on-write flags, output
tracing, GC sources, and parse-boundary hooks are unchanged.

The previous entry-only cap could retain 256 entire wide key graphs. The
separate parse-key cache clears above 4,096 keys, so successive wide parses
get different key pointers and miss the shape cache's identity comparison.
Those misses accumulated retained metadata and expensive GC root walks.

## Results

The [full matrix](results/cache/tables.md) compares the number-formatting
commit `0b5878547` with this cache change, using identical application code
and matched runtime build settings. The quiet Apple M1 run includes default
GC behavior and the same iteration counts for both arms and Node/Bun.

| Wide-object measure | Before | After | Change |
|---|---:|---:|---:|
| Parse CPU, 36 timed calls | 44.35 ms | 21.04 ms | 2.11× faster |
| Parse peak RSS, same loop | 226.95 MiB | 176.81 MiB | 50.14 MiB lower |
| RSS retaining 16 parsed results | 75.66 MiB | 71.27 MiB | 4.39 MiB lower |

The improvement is substantial, but this row still takes 4.26× Node's CPU
and 5.07× Bun's CPU. Full parity remains unachieved. [Every CPU/RSS
target](results/cache/parity.md) remains visible; 5 of 38 CPU medians meet the
better engine, 58 of 74 peak RSS medians do, and 28 of 36 retained RSS medians
do. This inventory is not a statistical or correctness completion gate.

Most other CPU rows are within roughly 1% of the reference. The full matrix
initially showed a 4.8% slowdown for stringify of `"a"`. A separate quiet,
seven-process-per-engine [repeat](results/cache/recheck-string/timing.jsonl)
did not reproduce it: median 0.072248 → 0.071674 µs, with all seven candidate
samples below the reference median. The [repeat window](results/cache/recheck-string/window.json)
and original data are both preserved. The goal's broad no-regression claim
still requires verification across the final combined implementation.

## Validation and scope

- 47 JSON runtime tests pass, including a test that fills the retained-key
  budget with medium shapes, checks small-shape reuse, and verifies wide
  shapes are not cached.
- 23 compiled fixtures match Node, including repeated 5,000-key parses that
  keep selected outputs alive, then mutate and re-stringify them.
- That new lifetime fixture passes moving-GC stress seeds 17/9013 with
  from-space protection and evacuation verification: 13/17 copying minors
  move 19,880/26,542 objects. The existing scanning fixture also passes both
  seeds, with positive collection/movement counters.
- All 152 benchmark output checks, 456 timing trials and 432 memory trials
  succeed; the quiet-window gate passes.

The [validation record](results/cache/validation.json), [archive/source
provenance](results/cache/provenance.json), and [quiet window](results/cache/window.json)
contain the evidence. This bounds one source of retained parser metadata;
it does not establish strict memory neutrality for all existing runtime
caches or remove the cost of tracing live input/output graphs.

Next candidates already have leaf-level correctness evidence: faster UTF-16
length counting and faster formatting of compact decimals. They have not yet
been integrated or measured in the full runtime. Tiny-call overhead and the
remaining general object traversal costs still need work.

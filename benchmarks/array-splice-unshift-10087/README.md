# Array splice/unshift layout evidence (#10087)

This directory preserves the three standalone issue reproducers and the raw
before/after results. Both sweeps used the unchanged TypeScript sources,
Node v26.5.1, matching release-built Perry artifacts, a 60-second timeout per
process, and a quiet Linux x86_64 host. The JSON artifacts record the exact
source, compiler, runtime, stdlib, and Node hashes, plus all seven raw samples
behind every reported median.

## Result

Perry milliseconds per workload invocation:

| workload | n | main `50e08e91dd` | candidate `51143d8dd8` |
| --- | ---: | ---: | ---: |
| middle remove | 100 | 0.011 | 0.009 |
| | 1,000 | 0.568 | 0.110 |
| | 10,000 | 50.089 | 2.477 |
| | 100,000 | 4,943.798 | 177.435 |
| middle insert | 100 | 0.012 | 0.010 |
| | 1,000 | 0.559 | 0.109 |
| | 10,000 | 50.121 | 2.387 |
| | 100,000 | 4,924.992 | 167.874 |
| unshift build | 100 | 0.010 | 0.005 |
| | 1,000 | 0.552 | 0.072 |
| | 10,000 | 50.885 | 3.522 |
| | 100,000 | TIMEOUT | 314.457 |

Every completed Perry checksum matches Node. All candidate processes complete
100,000 operations. Over the shared 1,000-100,000 range, the log/log slopes
and Perry-minus-Node deltas are:

| workload | Node slope | Perry slope | delta |
| --- | ---: | ---: | ---: |
| middle remove | 1.900 | 1.604 | -0.296 |
| middle insert | 1.835 | 1.593 | -0.242 |
| unshift build | 1.877 | 1.821 | -0.056 |

## Mechanism and bounded work

The dense fast paths still pay their required overlapping element move. They
no longer reclassify every live slot afterward. The finisher instead:

- classifies and barriers only newly inserted values;
- retains exact pointer-free or all-pointer metadata when the insert permits;
- drops position-specific mixed metadata to conservative UNKNOWN in constant
  time;
- translates old-generation dirty-page coverage to the moved destination; and
- always revokes the conservative element-shape proof.

Runtime unit counters cover repeated unshift, middle insertion, and middle
removal. For `n` operations they observe exactly `n` classified layout slots,
including the one-element deleted arrays produced by repeated removal, rather
than the previous sum of all live receiver lengths.

Moving-GC tests promote an array, insert young pointers with both operations,
run a copying minor and then a full collection, and validate the rewritten
children. Old-array witnesses move old-to-young edges across remembered-set
page boundaries in both directions. A forced-evacuation splice test also proves
that its receiver and caller-provided pointer items are rooted before species
allocation. Existing splice/unshift element-shape sabotage tests continue to
prove that mixed-kind replacement cannot retain a stale proof.

## Reproduce

Build Perry and its matching static libraries in the checkout under test, then
run:

```sh
python3 benchmarks/array-splice-unshift-10087/run.py \
  --perry target/release/perry \
  --node /path/to/node-v26.5.1/bin/node \
  --output benchmarks/array-splice-unshift-10087/result.json
```

The runner compiles all three sources with auto-optimization and the compile
cache disabled, executes sizes 100, 1,000, 10,000, and 100,000 sequentially,
checks cross-engine checksums, and records both general and acceptance-range
slopes.

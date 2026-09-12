# Array splice/unshift layout evidence (#10087)

This directory preserves the three standalone issue reproducers and the raw
before/after results. Both sweeps used the unchanged TypeScript sources,
Node v26.5.1, matching release-built Perry artifacts, a 60-second timeout per
process, and a quiet Linux x86_64 host. The JSON artifacts record the exact
source, compiler, runtime, stdlib, and Node hashes.

## Result

Perry milliseconds per workload invocation:

| workload | n | main `50e08e91dd` | candidate `ccf2e64dd8` |
| --- | ---: | ---: | ---: |
| middle remove | 100 | 0.012 | 0.006 |
| | 1,000 | 0.579 | 0.082 |
| | 10,000 | 51.293 | 2.203 |
| | 100,000 | TIMEOUT | 173.495 |
| middle insert | 100 | 0.013 | 0.007 |
| | 1,000 | 0.581 | 0.080 |
| | 10,000 | 51.822 | 2.075 |
| | 100,000 | TIMEOUT | 162.663 |
| unshift build | 100 | 0.010 | 0.005 |
| | 1,000 | 0.560 | 0.076 |
| | 10,000 | 51.206 | 3.578 |
| | 100,000 | TIMEOUT | 314.602 |

Every completed Perry checksum matches Node. All candidate processes complete
100,000 operations. Over the shared 1,000-100,000 range, the log/log slopes
and Perry-minus-Node deltas are:

| workload | Node slope | Perry slope | delta |
| --- | ---: | ---: | ---: |
| middle remove | 1.930 | 1.662 | -0.268 |
| middle insert | 1.841 | 1.653 | -0.187 |
| unshift build | 1.880 | 1.808 | -0.073 |

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
children. A separate old-array witness moves an old-to-young edge across a
remembered-set page boundary. Existing splice/unshift element-shape sabotage
tests continue to prove that mixed-kind replacement cannot retain a stale
proof.

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

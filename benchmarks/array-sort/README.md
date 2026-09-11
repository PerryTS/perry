# Generic comparator sort benchmark

The harness feeds identical inputs to Node and matching baseline/candidate Perry compiler/runtime builds. Every output element, value type, and stable object order is checked against Node. JSON loading, array construction, verification, and serialization are outside the sort timer; comparator construction is included.

```sh
python3 benchmarks/array-sort/run.py --baseline /path/to/baseline-build \
  --candidate /path/to/candidate-build --samples 9 --warmups 2 \
  --output /tmp/array-sort-results
```

Each build directory must contain its matching `perry`, `libperry_runtime.a`, and `libperry_stdlib.a`. Build both revisions with the same flags and package set; keep runtime coherence checks enabled. Options include `--size`, `--samples`, `--warmups`, `--node`, and `--cases all|matrix|issue`.

`issue-289.ts` reproduces the original negative-number input from [scriptc #289](https://github.com/vercel-labs/scriptc/issues/289), including first-sort initialization, and verifies every element. It is distinct from the matrix's positive descending fixture and runs by default.

## M1 Max results, 2026-09-11

Baseline: `1a9c0de6cb790d2467b0ca22a660870025179b37` (Perry 0.5.1531). Candidate: this PR's compiler/runtime optimizations. Node v26.5.1. Both Perry builds use release optimization, 16 codegen units, and LTO off. All rows contain 100,000 elements; medians of nine randomized interleaved fresh-process samples after two warmups per engine. Other user workloads were active on this shared host: retain the raw sample ranges when interpreting close results.

The original issue measures **90.97 → 0.61 ms**, compared with **Node 1.68 ms** (2.75× faster than Node). Perry beats Node in **24/25 cases** in this run. This is a local benchmark result, not a claim that all JavaScript runs faster than Node.

| Input | Values | Baseline ms | Perry ms | Node ms | Node / Perry |
| --- | --- | ---: | ---: | ---: | ---: |
| random | number | 75.02 | 15.86 | 17.96 | 1.13× |
| random | object | 104.57 | 24.03 | 21.62 | 0.90× |
| random | string | 241.75 | 28.55 | 37.68 | 1.32× |
| sorted | number | 30.27 | 0.62 | 1.24 | 2.00× |
| sorted | object | 38.08 | 0.88 | 1.31 | 1.49× |
| sorted | string | 91.19 | 1.33 | 2.86 | 2.15× |
| reverse | number | 87.94 | 0.63 | 1.33 | 2.10× |
| reverse | object | 121.69 | 0.90 | 1.38 | 1.53× |
| reverse | string | 275.85 | 1.31 | 1.80 | 1.37× |
| equal | number | 29.60 | 0.58 | 1.19 | 2.05× |
| equal | object | 37.71 | 0.85 | 1.27 | 1.50× |
| equal | string | 90.22 | 1.87 | 1.99 | 1.06× |
| duplicates | number | 60.55 | 6.49 | 9.59 | 1.48× |
| duplicates | object | 83.01 | 9.56 | 10.76 | 1.13× |
| duplicates | string | 226.06 | 12.45 | 14.63 | 1.17× |
| runs | number | 34.48 | 0.86 | 2.53 | 2.94× |
| runs | object | 45.64 | 1.23 | 2.60 | 2.12× |
| runs | string | 100.67 | 1.36 | 3.67 | 2.70× |
| nearly_sorted | number | 32.14 | 1.07 | 2.66 | 2.49× |
| nearly_sorted | object | 43.93 | 1.58 | 2.89 | 1.83× |
| nearly_sorted | string | 99.56 | 1.88 | 4.22 | 2.24× |
| organ_pipe | number | 58.71 | 0.89 | 2.09 | 2.35× |
| organ_pipe | object | 79.58 | 1.45 | 2.15 | 1.48× |
| organ_pipe | string | 172.57 | 1.96 | 3.19 | 1.63× |
| issue_289 | number | 90.97 | 0.61 | 1.68 | 2.75× |

Ratios above 1 favor Perry. Random objects remain 11% slower than Node. A separate 15-sample interleaved comparison against the preceding PR build retains this gap and records modest gains from removing full-cache loads; those raw samples are included as well. [measured-m1-max.json](measured-m1-max.json) contains every raw timing, executable/archive hashes, source hashes, the harness hash, and build flags. Local build paths are replaced with arm names.

## What changed

The former comparator sort insertion-sorted fixed 32-element chunks and merged values through rooted arrays. Profiles showed excessive comparisons on ordered input and repeated array-layout, slot-tracking, and write-barrier work inside comparison loops.

The adaptive engine sorts integer indices into an immutable rooted snapshot. It detects natural runs, reverses only strictly descending runs, extends short runs with stable binary insertion, balances merges, and searches blocks after consecutive wins. Values are published once; the dense path consumes the permutation directly. Exotic receivers retain property operations, strict writes/deletes, and current roots across getters, setters, and callbacks.

Native stack root cells are bound to the moving collector and reread before each callback. Small index workspaces use the stack; larger workspaces use rooted, non-moving Uint32Array backing storage, which remains reclaimable when a JavaScript exception skips Rust destructors. Array numeric-layout reconstruction scans once and rebuilds remembered edges in bulk after callback-free copies.

Shared compiler/runtime improvements reduce dynamic numeric-tag checks, primitive-string coercion, short ASCII comparison work, and closure dispatch. Generic property reads use an atomic compact ShapeId/slot MRU with live receiver-kind and descriptor guards; the full cache pointer is loaded only after this compact path fails, and polymorphic, overflow, proxy, and prototype handling remains available on misses. This adds eight bytes per inline property-read site. Cache-miss and marking-barrier calls carry a code-layout hint while retaining their full memory/GC effects.

All optimizations apply to ordinary operations and arbitrary comparators. The compiler does not recognize sort comparator bodies, substitute extracted keys, skip output checks, or defer collections. Non-ASCII comparisons retain UTF-16 ordering, and comparator results retain abstract ToNumber semantics, including BigInt/Symbol errors.

## Validation

- The complete serialized suites pass: 1,465 compiler tests and 3,537 runtime tests (five ignored).
- Runtime witnesses force actual movement during comparisons, stack-root buffer growth, and collection getters. The new getter regression failed with wrong output before the rooting fix and passes after it, asserting relocation of both closure and receiver.
- Four compiled TypeScript regressions match Node normally and with seeded copying GC, evacuation verification, and protected from-space. Coverage includes mixed numeric tags, strings across word boundaries and UTF-16 cases, changing property-cache shapes, overflow slots, descriptors, proxies, stable identities, holes, array-like receivers, `toSorted`, allocating coercion/setters, mutation, reentrancy, and exceptions.
- All 25 benchmark cases verify complete output against Node on every sample.
- Address classification, GC store inventory, root-holder/poll-reach checks, raw-handle debt, file-size, formatting, and shape-descriptor census gates pass. The census follows both cache entry points into their shared implementation and includes sabotage tests for bypassed authority and invalid ShapeIds. These source audits complement the runtime witnesses; source markers alone are not proof of moving-GC correctness.

Linux CI also passed the actual-movement sort/root-cell witnesses. CI on the preceding head reported a stale shape-census assumption; this follow-up updates that checker and its sabotage tests. Other failures remain: Locally reproduced gap cases have the same outcomes on pristine baseline and earlier follow-up candidates. The public benchmark freshness check also fails on baseline; the Linux stack-size assertion requires Linux verification. See the PR for current CI status rather than interpreting local checks as a green CI run.

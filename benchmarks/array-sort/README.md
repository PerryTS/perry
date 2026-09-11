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

The original issue measures **104.37 → 0.67 ms**, compared with **Node 2.06 ms** (3.05× faster than Node). Perry beats Node in **24/25 cases** in this run. This is a local benchmark result, not a claim that all JavaScript runs faster than Node.

| Input | Values | Baseline ms | Perry ms | Node ms | Node / Perry |
| --- | --- | ---: | ---: | ---: | ---: |
| random | number | 78.34 | 17.15 | 21.56 | 1.26× |
| random | object | 128.83 | 29.62 | 26.61 | 0.90× |
| random | string | 332.73 | 28.92 | 39.49 | 1.37× |
| sorted | number | 35.09 | 0.60 | 1.37 | 2.29× |
| sorted | object | 40.04 | 0.97 | 1.50 | 1.55× |
| sorted | string | 74.62 | 1.09 | 2.42 | 2.21× |
| reverse | number | 124.82 | 0.64 | 1.53 | 2.37× |
| reverse | object | 140.59 | 0.97 | 1.60 | 1.65× |
| reverse | string | 336.58 | 1.38 | 2.32 | 1.67× |
| equal | number | 33.15 | 0.59 | 2.29 | 3.84× |
| equal | object | 46.65 | 0.92 | 2.79 | 3.03× |
| equal | string | 117.44 | 2.04 | 2.75 | 1.35× |
| duplicates | number | 74.51 | 6.79 | 12.29 | 1.81× |
| duplicates | object | 100.49 | 13.23 | 13.96 | 1.06× |
| duplicates | string | 273.86 | 13.45 | 20.89 | 1.55× |
| runs | number | 40.68 | 0.87 | 4.83 | 5.56× |
| runs | object | 56.68 | 1.29 | 3.39 | 2.63× |
| runs | string | 123.60 | 1.45 | 4.31 | 2.98× |
| nearly_sorted | number | 40.29 | 1.12 | 3.67 | 3.27× |
| nearly_sorted | object | 52.19 | 1.81 | 4.55 | 2.51× |
| nearly_sorted | string | 122.74 | 1.99 | 4.93 | 2.48× |
| organ_pipe | number | 65.97 | 1.00 | 4.04 | 4.05× |
| organ_pipe | object | 96.37 | 1.71 | 3.27 | 1.91× |
| organ_pipe | string | 224.78 | 2.41 | 3.54 | 1.47× |
| issue_289 | number | 104.37 | 0.67 | 2.06 | 3.05× |

Ratios above 1 favor Perry. Random objects remain 11% slower than Node; the 6% duplicate-object margin is small enough to warrant checking on an idle host. An additional 15-sample issue-only run measured Perry 0.67 ms and Node 3.81 ms as host load shifted; the table keeps the complete-matrix run. [measured-m1-max.json](measured-m1-max.json) contains every raw timing, executable/archive hashes, source hashes, the harness hash, and build flags. Local build paths are replaced with arm names.

## What changed

The former comparator sort insertion-sorted fixed 32-element chunks and merged values through rooted arrays. Profiles showed excessive comparisons on ordered input and repeated array-layout, slot-tracking, and write-barrier work inside comparison loops.

The adaptive engine sorts integer indices into an immutable rooted snapshot. It detects natural runs, reverses only strictly descending runs, extends short runs with stable binary insertion, balances merges, and searches blocks after consecutive wins. Values are published once; the dense path consumes the permutation directly. Exotic receivers retain property operations, strict writes/deletes, and current roots across getters, setters, and callbacks.

Native stack root cells are bound to the moving collector and reread before each callback. Small index workspaces use the stack; larger workspaces use rooted, non-moving Uint32Array backing storage, which remains reclaimable when a JavaScript exception skips Rust destructors. Array numeric-layout reconstruction scans once and rebuilds remembered edges in bulk after callback-free copies.

Shared compiler/runtime improvements reduce dynamic numeric-tag checks, primitive-string coercion, short ASCII comparison work, and closure dispatch. Generic property reads use an atomic compact ShapeId/slot MRU with live receiver-kind and descriptor guards; polymorphic, overflow, proxy, and prototype handling remains available on misses. This adds eight bytes per inline property-read site. Cache-miss and marking-barrier calls carry a code-layout hint while retaining their full memory/GC effects.

All optimizations apply to ordinary operations and arbitrary comparators. The compiler does not recognize sort comparator bodies, substitute extracted keys, skip output checks, or defer collections. Non-ASCII comparisons retain UTF-16 ordering, and comparator results retain abstract ToNumber semantics, including BigInt/Symbol errors.

## Validation

- The complete serialized suites pass: 1,465 compiler tests and 3,537 runtime tests (five ignored).
- Runtime witnesses force actual movement during comparisons, stack-root buffer growth, and collection getters. The new getter regression failed with wrong output before the rooting fix and passes after it, asserting relocation of both closure and receiver.
- Four compiled TypeScript regressions match Node normally and with seeded copying GC, evacuation verification, and protected from-space. Coverage includes mixed numeric tags, strings across word boundaries and UTF-16 cases, changing property-cache shapes, overflow slots, descriptors, proxies, stable identities, holes, array-like receivers, `toSorted`, allocating coercion/setters, mutation, reentrancy, and exceptions.
- All 25 benchmark cases verify complete output against Node on every sample.
- Address classification, GC store inventory, root-holder/poll-reach checks, raw-handle debt, file-size, and formatting gates pass. These source audits complement the runtime witnesses; source markers alone are not proof of moving-GC correctness.

CI on the prior PR head has unrelated failures. Locally reproduced gap cases have the same outcomes on pristine baseline and earlier follow-up candidates. The public benchmark freshness check also fails on baseline; the Linux stack-size assertion requires Linux verification. See the PR for current CI status rather than interpreting local checks as a green CI run.

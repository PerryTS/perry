# Generic comparator sort benchmark

The harness feeds identical pre-generated inputs to Node and two matching Perry compiler/runtime builds. It validates every sorted element and stable object order against Node before recording a timing. Input generation is outside both measured engines; JSON loading, array construction, verification, and output serialization are outside the sort timer.

```sh
python3 benchmarks/array-sort/run.py --baseline /path/to/baseline-build \
  --candidate /path/to/candidate-build --output /tmp/array-sort-results
```

Each build directory must contain its matching `perry`, `libperry_runtime.a`, and `libperry_stdlib.a`. Keep compiler sources and build flags identical for a runtime A/B. Runtime coherence checks remain enabled. Options include `--size`, `--samples`, `--warmups`, and `--node`.

## M1 Max results, 2026-09-11

Baseline: `1a9c0de6cb790d2467b0ca22a660870025179b37` (Perry 0.5.1531). Candidate: this adaptive index-sort change. Node 26.5.1. Both Perry builds use release optimization, 16 codegen units, and LTO off. Each row is 100,000 elements; medians of seven randomized interleaved fresh-process samples after two warmups per engine. These are local measurements, not a claim about every workload or platform.

| Input | Values | Baseline ms | Candidate ms | Speedup | Node ms |
| --- | --- | ---: | ---: | ---: | ---: |
| random | number | 81.39 | 30.59 | 2.66× | 18.62 |
| random | object | 123.84 | 43.35 | 2.86× | 26.94 |
| random | string | 252.17 | 133.57 | 1.89× | 39.13 |
| sorted | number | 30.33 | 3.23 | 9.38× | 1.40 |
| sorted | object | 40.49 | 3.51 | 11.54× | 1.40 |
| sorted | string | 74.65 | 7.72 | 9.67× | 2.36 |
| reverse | number | 92.48 | 3.27 | 28.29× | 1.38 |
| reverse | object | 127.21 | 3.53 | 36.07× | 1.45 |
| reverse | string | 290.97 | 11.27 | 25.81× | 1.87 |
| equal | number | 30.77 | 3.21 | 9.58× | 1.34 |
| equal | object | 39.65 | 3.51 | 11.31× | 1.38 |
| equal | string | 94.26 | 11.79 | 8.00× | 2.08 |
| duplicates | number | 63.37 | 14.80 | 4.28× | 10.12 |
| duplicates | object | 91.75 | 19.23 | 4.77× | 11.32 |
| duplicates | string | 258.75 | 80.48 | 3.22× | 16.31 |
| runs | number | 36.52 | 3.65 | 10.01× | 2.64 |
| runs | object | 53.34 | 4.53 | 11.77× | 3.60 |
| runs | string | 113.78 | 11.06 | 10.29× | 4.55 |
| nearly_sorted | number | 35.35 | 5.01 | 7.05× | 3.72 |
| nearly_sorted | object | 49.63 | 5.01 | 9.90× | 4.34 |
| nearly_sorted | string | 117.42 | 14.38 | 8.17× | 4.73 |
| organ_pipe | number | 79.38 | 5.84 | 13.59× | 3.42 |
| organ_pipe | object | 98.79 | 7.16 | 13.80× | 2.38 |
| organ_pipe | string | 203.79 | 19.66 | 10.37× | 3.83 |

All 24 matrix rows improve, by 1.89–36.07×. Node still leads all rows in this run; the remaining gap depends strongly on the comparator and input. See [measured-m1-max.json](measured-m1-max.json) for raw samples, executable/archive hashes, and candidate source hashes. Local build directory paths have been replaced with arm names.

## What changed

The former comparator sort insertion-sorted fixed 32-element chunks and merged GC values through two rooted arrays. A sample profile of the released implementation showed repeated `note_array_slot`, `array_numeric_layout`, `layout_note_slot`, and write-barrier work in its sorting loops, in addition to excessive comparisons on ordered inputs.

The replacement sorts integer indices into one immutable rooted snapshot. Natural ascending and strictly descending runs, stable binary insertion, balanced merges, and exponential/binary block searches work with arbitrary comparators. During callbacks, only indices move; operands and the closure are read through current roots. After callbacks finish, a cycle permutation writes values once and rebuilds the layout and remembered edges. The initial snapshot likewise uses a copy followed immediately by a rebuild, with no intervening safepoint.

Small index workspaces live on the native stack. Larger workspaces are rooted, non-moving Uint32Arrays, so JS throws can skip Rust destructors without leaking native Vec allocations. No comparator-body recognition, numeric-only branch, GC deferral, or benchmark-specific switch is used.

## Validation

- 300 array-related runtime tests, including seven new algorithm tests, pass.
- 34 GC callback tests pass; the strengthened sort test forces copying during comparisons and asserts actual relocation across stack and heap workspace sizes.
- The compiled TypeScript regression verifies stable ordering, actual object reference identity, mixed pointers/numbers, strings, holes, array-like receivers, toSorted, coercion, mutation, reentrancy, exceptions, and recovery, against Node.
- Compiled regression also passes with seeded copying GC, evacuation verification, and protected from-space: 1,049 copying minors and 34,249 relocated objects.
- An additional complete matrix pass compares canonical JSON, so Python's `True == 1` cannot hide a wrong JS value type.
- Address-classification, GC store-site inventory, raw-handle debt, file-size, and formatting checks pass. The pre-existing `relevant_box_roots` warning is present in both baseline and candidate builds.

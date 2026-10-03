# Local measurements, 2026-10-02

Apple M1 Max, macOS arm64; Node 26.5.1. Baseline: main
`47a0c7bf1795`, v0.5.1655. Candidate: the same compiler sources with
`MIN_ARRAY_CAPACITY` reduced from 16 to four. Each arm rebuilt the compiler,
`perry-runtime-static` and `perry-stdlib-static` together with the release
profile and `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`; each executable linked
its own preserved archives through `PERRY_RUNTIME_DIR`, with
`--no-auto-optimize --no-cache`. No version bump or compiler lowering change.
Raw samples and census summaries are in `results.json`.

## Original cyclic workload

Three sequential repeats per arm, alternating order each round;
`PERRY_GC_HEAP_LIMIT=1024`. Every run matched Node's full stdout and exit
status, including the workload's parent and closure/cache identity checks.
CPU is child user + system time; RSS is peak resident memory from `wait4`.
These runs use the unchanged source, without census instrumentation or
explicit GC. This is a shared development host, not an isolated performance
runner; no repeatable CPU improvement is claimed.

| Median | Baseline | Four slots | Change |
|---|---:|---:|---:|
| CPU seconds | 11.472063 | 11.766732 | +2.6% |
| Peak RSS MiB | 131.390625 | 125.937500 | -4.2% |

A separate diagnostic copy adds exactly six full-GC census points (checkpoint
7, 55, 103, `released`, 199, and `released-again`). It changes scheduling and
allocates observer buffers; its RSS is not substituted for the original run.
At every populated-cache census (four checkpoints):

| Live arrays | Baseline | Four slots |
|---|---:|---:|
| Count | 10,922 | 10,922 |
| Tracked allocation bytes, including headers | 1,572,672 | 524,288 |
| Element capacity slots | 174,740 | 43,692 |
| Used slots | 10,924 | 10,924 |
| Unused element bytes | 1,310,528 | 262,144 |

Thus live array storage drops about 66.7%, saving approximately 1 MiB.
Four slots deliberately retain a small amount of spare backing for leaves:
four-child nodes fill without allocating a second backing/forwarding stub.
After release, both arms retain just two internal arrays and four used slots.

## Growing arrays: the cost of less initial headroom

Same measurement protocol and exact Node output checks. The short cases build
one million independent arrays; the large case builds 100 arrays.

| Final width | Baseline CPU s | Four-slot CPU s | Baseline RSS MiB | Four-slot RSS MiB |
|---|---:|---:|---:|---:|
| 4 | 0.052199 | 0.043043 | 12.531250 | 12.546875 |
| 5 | 0.050685 | 0.147338 | 12.546875 | 13.078125 |
| 16 | 0.078358 | 0.289327 | 12.531250 | 15.203125 |
| 100,000 | 0.080258 | 0.075425 | 58.328125 | 59.078125 |

The five-element case pays one extra growth; the sixteen-element case pays
two (4 -> 8 -> 16). They are about 2.9x and 3.7x slower respectively in this
allocation-heavy microbenchmark. The large case amortizes those extra grows.
This is a memory/short-growth tradeoff, not a universal throughput improvement;
callers knowing their final width can already request it explicitly.

## Correctness and moving GC

- All 4,759 runtime unit tests pass (five ignored), including the new four-slot sizing and
  independent-identity test, growth forwarding, hole tails, named properties,
  and subclass suites. The strict-store and spill fixtures now explicitly
  reserve the dense slots/headroom they need. The mixed-concat trace test
  exercises both the roomy masked destination and a small tag-scanned one.
  The nested structured-clone test quarantines retired memory so address
  recycling cannot make its relocation assertion fail.
- The new old-array test appends 20 young strings, grows through the initial
  capacity, and forces a copying minor after each append. It asserts that the
  new string actually moves and every string remains reachable through the
  array, including its original forwarding alias.
- `test_gap_small_array_capacity.ts` matches Node with defaults and at every
  loop poll under seed 11744, rate 1, allocation stride 0, protected from-space
  and the evacuation verifier: 80 copying minors, 15,419 moved objects.
- The full unchanged cyclic workload matches Node under seed 11744, rate 0.01,
  allocation stride 64 KiB, configured nursery 4 MiB, protected from-space,
  evacuation verification and array-hole verification: 926 copying minors,
  6,350,342 moved objects. This stress run uses the default runtime heap budget;
  the uninstrumented timing runs use the explicit 1 GiB budget above.

The local script lint suite has an existing public-baseline freshness failure.
All of its fingerprinted source/harness inputs are unchanged from the PR base.
The new test's raw-handle debt finding was fixed with scoped handle APIs and
the debt checks rerun. The separate compile lint tier was skipped; release
compiler/archive builds and scoped runtime tests were run directly.

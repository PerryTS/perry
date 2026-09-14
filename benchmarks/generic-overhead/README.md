# Generic function overhead

These exported probes measure the fixed instruction cost of local copies,
registered global stores, and calls through a function parameter. They count
ARM64 machine instructions after LLVM optimization, including cold blocks and
excluding callees. They do not measure executed instructions or throughput.

## Reproduce

Build the reference and candidate compilers from their own checkouts. No runtime
archives are needed for this object-only census.

```sh
python3 benchmarks/generic-overhead/census.py \
  --before /path/to/reference/perry \
  --after /path/to/candidate/perry \
  --objdump /path/to/llvm-objdump \
  --out /tmp/perry-generic-overhead-census
```

The default is Perry's LLVM `-Os` policy. Repeat with `--opt 3` to check `-O3`.
Each arm retains the compiler log, traced LLVM IR, object, and disassembly. The
JSON report records compiler and source hashes alongside the instruction counts.

## Measured instruction counts

Measured against `4945fc1f7498debc76e9f861d7cf1517da5e67c9` on an Apple M1 Max,
using LLVM 22.1.4 and Perry's `apple-m1` target. Both columns count the same
exported probe source; the callback and unknown-value store are controls.

| Probe | `-Os` before → after | `-O3` before → after |
| --- | ---: | ---: |
| Identity | 3 → 3 | 3 → 3 |
| One local alias | 21 → 3 | 24 → 3 |
| Three local aliases | 43 → 3 | 58 → 3 |
| Dead inert assignments | 43 → 3 | 58 → 3 |
| Alias live across a call | 70 → 54 | 79 → 55 |
| Constant global write | 11 → 6 | 12 → 7 |
| Declared-number global write wrapper | 17 → 17 | 22 → 22 |
| Unknown-value global write fallback | 17 → 17 | 17 → 17 |
| Callback through a function parameter | 97 → 97 | 107 → 107 |

Compiler SHA-256:

- Reference: `e5bbbf453f5660f53c54e8978ee10569daa00000489b5a5be09fd61ad7ee42e5`
- Candidate: `39f2a71d54fee3ca253caa3a372121e49453f423dad356ca153acf0fd3562a72`
- Probe source: `fcdab55841ae899da01a6b6d32f1f753ff753f1706dd5e12765a1eddd0b973fc`

## Optimization boundaries

- Copy cleanup runs after the HIR shape passes and before codegen emits string
  sharing and root shading. It forwards initialized, unwritten local bindings
  and removes unused inert stores. It excludes captures, control flow, TDZ
  preallocation, parameter defaults, arguments objects, and unsupported HIR
  expressions. Effectful discarded assignments keep both their RHS and storage.
- Global and static-field stores reuse the existing construction proof used for
  precise local roots. Proven scalar stores remain stores to registered roots;
  unknown values and declared-only numeric parameters retain incremental shading.

This does not change leaf-root laundering, rest/arguments materialization,
callback dispatch, closure allocation/boxing, borrowed global-string reads, or
cross-module GC effect propagation.

## Semantic coverage

`test-files/test_gap_generic_function_overhead.ts` compares with the pinned Node
oracle. It covers primitive and heap aliases, source writes, retained captures,
mapped arguments, TDZ, effectful assignments, declared-only number stores,
ordinary/arrow/bound/rest/proxy callbacks, exception propagation, and an object
alias kept live across allocating calls. Run the same executable under moving
GC stress and require nonzero copying collections and moved objects:

```sh
PERRY_GC_SCHEDULE_SEED=37 PERRY_GC_SCHEDULE_RATE=0.2 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
PERRY_GC_VERIFY_EVACUATION=1 /path/to/compiled/fixture
```

The crate unit tests separately assert removed work and required fallback IR;
an optimized-away fixture cannot satisfy those emission checks.

The final fixture matches Node 26.5.1 in both native and shadow-root modes,
normally and under the stress settings above. Each stress run performs 1,411
copying minor collections and moves 91,275 objects. The shadow IR passes both
the moving-root dominance check (180 root stores) and unrooted-alloca check
(162 GC-capable allocas), with zero violations.

## Callback follow-up

The callback probe is an unchanged control. A trial that routed function-typed
locals through `js_closure_call1_receiverless` reduced its caller from 97 to 9
instructions, but increased median CPU time for ordinary callbacks by 54%.
Arrow callbacks improved by 11%. This used five interleaved before/after pairs
of 20 million calls on the same host and matching runtime build profiles.
The trial moved receiver save/root/restore work into the runtime, so the smaller
caller alone was insufficient evidence. That routing change is excluded; optimizing
the ordinary-function runtime path remains follow-up work.

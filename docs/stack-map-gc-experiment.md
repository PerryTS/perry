# Stack-map GC experiment

> **Historical experiment.** The production GC/rooting source of truth is the
> maintained material under `docs/src/internals/`; the explicit statepoint
> follow-up is recorded in `statepoint-gc-experiment.md`. Do not treat this
> experiment's interim decisions as current architecture.

Date: 2026-07-31

Branch: `exp/stackmap-viability`

Base commit: `e2557c1a985cb983ed00aafd1a2c1b31f1570b98`

## Decision

LLVM stack maps are technically viable for Perry's moving collector on
macOS/arm64, but this prototype does **not** justify replacing the shadow stack
yet.

- Correctness is promising: all eight GC-ratchet probes match Node, forced
  evacuation verification passes, and retained heap is byte-for-byte identical
  to the shadow-stack arm.
- Runtime performance is effectively flat on ordinary workloads. The GC suite
  measured a noisy 1.5% geometric-mean improvement, while an interleaved
  deep-stack run was 1.5% slower. The host was heavily loaded, so neither is a
  defensible headline win.
- Compilation time is flat. Root-heavy executables gain an approximately
  16 KiB Mach-O segment even when the stack-map payload is only 2–9 KiB.
- Root enumeration is not simpler overall. The heap-backed frame stack and its
  TLS traffic can disappear, but they are replaced by LLVM ABI parsing,
  linker-retention rules, native unwinding, call-site matching, compiler memory
  barriers, and control-flow liveness analysis.
- The direction is strategically useful, but a production follow-up should
  investigate `gc.statepoint`/`gc.relocate`, not promote this plain
  `llvm.experimental.stackmap` prototype.

The prototype remains opt-in with `PERRY_STACK_MAPS=1`; the default
shadow-stack path is unchanged.

## What was built

The experiment reuses Perry's existing precise-root discovery and slot
numbering, then changes the backend:

1. Shadow-slot bind/clear operations remain temporary IR markers.
2. A final per-function pass resolves logical slots to their native allocas.
3. A conservative CFG dataflow pass computes roots that may be live at each
   call. A join uses union, so it may retain a stale value but cannot omit a
   live root.
4. Each call with live roots gets an `llvm.experimental.stackmap` carrying the
   addresses of those allocas. LLVM emits them as writable `Direct`
   frame-register-relative locations.
5. Empty memory-clobbering inline assembly brackets mapped calls. It generates
   no instructions, but makes the collector's otherwise invisible slot rewrite
   observable to LLVM before and after the call.
6. The runtime parses LLVM stack-map v3 records from the main Mach-O image,
   unwinds active frames with `_Unwind_Backtrace`, finds the record immediately
   preceding each return PC, and feeds its mutable slots into the existing GC
   root visitor.
7. Mach-O module assembly marks each local `__LLVM_StackMaps` atom
   `.no_dead_strip`; otherwise Perry's normal link removes the metadata.

The parser accepts linker-concatenated stack-map blobs and handles LLVM's
independent alignment before and after the live-out list. Both details caused
real failures during the spike and now have regression coverage.

LLVM documents this intrinsic and binary format as experimental and explicitly
separate from its GC statepoint machinery:
[Stack maps and patch points](https://llvm.org/docs/StackMaps.html) and
[Garbage collection safepoints](https://llvm.org/docs/Statepoints.html).

## Correctness results

The final release artifacts passed:

- all 8 GC-ratchet probes, with stdout identical to Node;
- all 8 probes again with `PERRY_GC_FORCE_EVACUATE=1` and
  `PERRY_GC_VERIFY_EVACUATION=1`;
- the string-retention probe 100 consecutive times after it exposed the
  parser/call-site issues;
- all 310 `perry-codegen` library tests;
- 1,570 `perry-runtime` library tests in single-threaded mode (3 ignored),
  excluding the existing debug-only `extern "C"` malformed-pop test whose
  intentional `debug_assert!` aborts a debug test process;
- 3 stack-map parser tests;
- 3 stack-map lowering/liveness tests;
- 46 object-cache tests, including `PERRY_STACK_MAPS` cache separation.

Across the full ratchet, median retained heap and heap capacity were identical
for every probe. Promotions were identical. Six probes copied exactly the same
number of objects; the remaining differences were +3, -6, and +2 objects, with
the same final retention and freed bytes.

The spike found three important correctness requirements:

- A moving collector must map writable native slots, not merely record pointer
  values. Passing alloca addresses produces LLVM `Direct` locations.
- Stack-map parsing must honor the pre-live-out alignment in the v3 format.
  Missing it desynchronized any record with an odd number of locations.
- A shadow-slot clear is a liveness change, not a write to the program local.
  Zeroing the native local corrupted a value used after its last GC-capable
  call. Static per-call liveness is required.

## Performance results

Hardware was an Apple M1 Max on macOS 26.5. The GC runs reported load averages
between 19 and 45, so the numbers below are directional only. Each A/B used the
same final compiler and runtime archives, with caches and auto-optimization
disabled. Runtime pairs were interleaved where noted.

### GC-ratchet wall time

Three measured runs plus one warmup per mode:

| Probe | Shadow | Stack map | Delta |
|---|---:|---:|---:|
| Nursery churn | 182.581 ms | 175.766 ms | -3.73% |
| Survivor promotion | 213.684 ms | 213.661 ms | -0.01% |
| Cross-generation writes | 210.161 ms | 203.122 ms | -3.35% |
| Dead after deep stack | 471.128 ms | 484.463 ms | +2.83% |
| Closure capture | 175.824 ms | 163.789 ms | -6.84% |
| String retention | 135.017 ms | 135.645 ms | +0.47% |
| Array grow/evacuate | 171.918 ms | 174.017 ms | +1.22% |
| Map/set side tables | 459.315 ms | 449.043 ms | -2.24% |

Geometric mean: stack maps were 1.50% faster. This is smaller than the
cross-run noise expected under the recorded host load. An additional
11-pair interleaved run measured:

- deep active stack: stack maps **1.49% slower**;
- string retention: stack maps **0.07% faster**.

The deep-stack result is consistent with native unwinding costing more than a
linear walk over the compact shadow buffer.

### Broader runtime samples

Eleven interleaved runs per binary:

| Workload | Shadow | Stack map | Delta |
|---|---:|---:|---:|
| Process startup | 5.351 ms | 5.238 ms | -2.10% |
| Method calls | 93.295 ms | 93.383 ms | +0.09% |
| Function calls | 111.325 ms | 111.055 ms | -0.24% |
| GC pressure | 38.958 ms | 38.874 ms | -0.22% |
| JSON roundtrip | 362.536 ms | 363.680 ms | +0.32% |

The substantive workloads are flat. Startup's 0.11 ms difference is below
what this host can resolve.

### Compile time and size

Five uncached compilations per cell:

| Workload | Shadow | Stack map | Delta |
|---|---:|---:|---:|
| Startup | 449.71 ms | 449.76 ms | +0.01% |
| Method calls | 472.72 ms | 471.16 ms | -0.33% |
| Function calls | 457.69 ms | 457.73 ms | +0.01% |
| GC pressure | 465.83 ms | 460.47 ms | -1.15% |
| JSON roundtrip | 486.94 ms | 489.84 ms | +0.59% |

The stack-map section measured 2,192 bytes for method calls, 5,512 bytes for GC
pressure, and 8,568 bytes for JSON roundtrip. On these Mach-O executables the
new segment rounded the file-size increase to roughly 16 KiB. Programs with no
mapped roots emitted no section and had identical file size.

The modified runtime archive is 18,656 bytes larger than the exact baseline
(30,313,640 versus 30,294,984 bytes).

## Is the GC simpler?

Only in a narrow sense.

The stack-map scanner is 451 lines in this spike versus 752 lines for the
current shadow-stack runtime. A completed replacement could also delete much
of the 563-line inline shadow-slot emitter and remove frame push/pop, TLS buffer
growth, longjmp depth restoration, and slot mirroring.

The total system is not yet simpler:

- the compiler gained a textual IR lowering and CFG liveness analysis;
- the linker needs platform-specific metadata retention;
- the runtime depends on an experimental LLVM binary contract and platform
  unwinder behavior;
- moving-GC relocation needs writable allocas plus compiler memory fences;
- diagnostics must distinguish missing metadata from a genuinely rootless
  frame;
- target support moves from ordinary generated calls to per-object-format
  section discovery and per-architecture DWARF register handling.

This trades locally optimized, explicit machinery for more cross-layer
machinery. It could become simpler after statepoints make relocation and
liveness first-class, but plain stack maps do not reach that point.

## Is this better positioned?

Potentially:

- per-safepoint liveness can be more precise than a mutable activation-wide
  registry;
- no shadow-frame push/pop or TLS slot mutation is required on the common
  path;
- native frame metadata aligns Perry with established AOT/JIT GC techniques;
- exceptions and non-local exits naturally remove unwound frames from the
  root set.

The current prototype is not yet a platform:

- scanning is implemented only for Mach-O/macOS;
- unknown calls remain conservatively instrumented; an audited call-effect
  table now omits runtime helpers proven unable to enter Perry's collector;
- the runtime assumes the matching stack-map PC is within 16 bytes of the
  unwound return PC;
- active roots must remain in addressable allocas;
- parameter/`this` bindings still need a fully audited incremental-mark
  transition barrier;
- async signals, foreign callbacks, tail calls, `setjmp`/`longjmp`, code
  splitting, and non-Apple object formats need dedicated end-to-end tests;
- `PERRY_STACK_MAPS=1` is currently unsafe on unsupported targets because the
  runtime scanner intentionally returns no roots there.

## Recommended next experiment

The explicit statepoint follow-up is now recorded in
[`statepoint-gc-experiment.md`](statepoint-gc-experiment.md). It validates
relocation correctness but finds no all-around performance win and no
whole-system simplification.

Do not replace the default shadow stack from this branch. If this direction
continues after the representation work:

1. Add an explicit safepoint capability table so pure calls do not receive
   metadata or optimization fences.
2. Close the incremental parameter/`this` barrier gap and add a probe that
   enters a new rooted activation during an in-flight incremental cycle.
3. Fail compilation on unsupported targets before expanding ELF/Windows
   section discovery and unwind/register support.
4. Re-run the A/B suite on an idle host with the repository's standard 11-run
   methodology and profile both mutator root updates and GC root scanning.

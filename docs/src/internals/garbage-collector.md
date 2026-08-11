# Garbage collector: current architecture and operations

> **Current as of 2026-08-11.** This is the source of truth for the collector
> that ships. The [generational plan][generational-plan] and
> [statepoint experiment][statepoint-experiment] are chronological
> evidence; their opening decisions describe the date they were written, not
> today's defaults.

Perry uses a per-thread, tracing generational collector. JavaScript values are
NaN-boxed, allocations carry an 8-byte `GcHeader`, and each runtime thread owns
a nursery plus an old-generation arena. The implementation is the
`crates/perry-runtime/src/gc/` and `crates/perry-runtime/src/arena/` module
trees; code generation's root lowering lives in
`crates/perry-codegen/src/codegen/` and `native_root_coverage/`.

## Collection paths

New GC-managed allocations normally enter 1 MiB nursery blocks. Two flag bits,
`HAS_SURVIVED` and `TENURED`, record age. A collection can take one of three
paths:

1. **Copying minor.** At a precise safepoint, live young objects are copied,
   roots and heap slots are rewritten, and whole from-space blocks are reset.
   Tenured survivors can move into `OLD_ARENA`. This is the fast nursery path.
2. **Non-moving minor/fallback.** When collection begins somewhere that cannot
   safely relocate every live reference, the nursery is marked and swept in
   place. Budgeted low-pause cycles also use a non-moving path.
3. **Full mark-sweep.** Major pacing, critical host pressure, explicit full
   work, or `PERRY_GEN_GC=0` trace both generations and reclaim dead old objects
   as well as nursery garbage.

`PERRY_GC_SCAVENGE` is on by default and lets nursery pressure route to the
direct minor. `PERRY_GC_SCAVENGE_NURSERY_MB` tunes its base high-water cap
(16 MiB by default); tenuring feedback may grow the effective cap. Generated
write barriers are also on by default. Turning them off makes generational
minors unsound, so the runtime deliberately falls back to full mark-sweep.

Old-generation page-defragmentation selection exists, but production
compaction is **off**. `PERRY_GC_OLD_DEFRAG=1` is a debugging/reproduction arm
while #7876 tracks the missing rewrite contract; nursery evacuation and normal
old-generation sweep are unaffected.

## Roots, by target

One root-set analysis feeds two lowerings:

| target | shipped precise-root lowering |
|---|---|
| 64-bit AArch64/arm64 and x86-64, including x86-64 Windows | LLVM RS4GC statepoints plus Perry's compact native stack map |
| `arm64_32` watchOS, ARM64 Windows, and unsupported architectures | Perry shadow frames |

This is target-aware, not host-aware. `PERRY_RS4GC=0` selects the shadow
lowering for bisection; `PERRY_RS4GC=1` requests native roots and fails closed
if the target cannot emit/read them. `PERRY_SHADOW_STACK=0` disables only the
shadow lowering—native-root analysis remains enabled when native roots are the
selected backend.

Runtime-owned roots do not live in generated frames. Registered scanners visit
module globals, pending async work, caches, registries, and other side tables;
`scripts/gc_runtime_root_holders.py` keeps the inventory complete. Runtime
helpers keep temporary values in `RuntimeHandleScope`/`RuntimeHandle` and must
re-read a handle after a call that can collect.

The conservative native-stack scan is not part of the production default:
`Auto` resolves to `SkipDisabled`. `PERRY_CONSERVATIVE_STACK_SCAN=full` is an
explicit diagnostic/sensitivity arm. A full scan pins ambiguous roots and
therefore makes the copying minor ineligible; it is useful evidence, not a
second normal rooting backend.

## Barriers and weak references

Every old-to-young pointer publication must hit the remembered-set barrier.
Codegen emits barriers for generated heap stores and runtime helpers perform
the same bookkeeping for their own exact stores. A minor traces remembered old
parents instead of retracing all of old-gen.

WeakRef, WeakMap, WeakSet, and FinalizationRegistry targets are excluded from
the strong trace. The copying minor processes only registered weak holders and
repairs forwarded addresses. Full/fallback collection currently walks the
whole arena during weak processing; that phase is atomic and unsliced (#7874).

## Budgets, memory pressure, and released blocks

The collector derives a heap budget, in priority order, from
`PERRY_GC_HEAP_LIMIT` (MiB), Apple embedded available-memory APIs, container
limits, then half of physical RAM. Budgets below 1 GiB scale trigger ceilings,
reclaim thresholds, nursery deferral slack, and RSS pressure thresholds down;
desktop/server defaults remain unchanged.

Platform hosts call `js_gc_memory_pressure(level)`:

- warning (`1`) requests a prompt minor;
- critical (`2+`) requests a full collection so old-gen garbage and idle arena
  blocks can be reclaimed;
- if collection is unsafe, the request is made sticky and drains at the next
  precise safepoint/allocation check.

Released 1 MiB blocks first enter a per-thread LIFO reuse pool capped at
64 MiB. Overflow is returned to the allocator and thread exit drains its own
pool. Critical pressure and small device budgets do not yet drain or resize an
already-populated pool; #7875 tracks that bounded-but-device-blind residue.

## Supported controls

These are the operational controls most useful outside collector development:

| knob | purpose |
|---|---|
| `PERRY_GEN_GC=0` | bisection fallback to full mark-sweep |
| `PERRY_WRITE_BARRIERS=0` | compile/runtime barrier bisection; also forces full mark-sweep |
| `PERRY_GC_SCAVENGE=0` | disable direct nursery scavenging for pacing comparison |
| `PERRY_GC_SCAVENGE_NURSERY_MB=N` | set the base nursery cap |
| `PERRY_GC_HEAP_LIMIT=N` | override the process heap budget in MiB |
| `PERRY_RS4GC=0` | select shadow roots on a native-root-capable target |
| `PERRY_CONSERVATIVE_STACK_SCAN=full` | diagnostic full native-stack scan; disables copying |
| `PERRY_GC_TRACE=1` | emit structured per-cycle trace records |
| `PERRY_GC_DIAG=1` | emit human-readable collector diagnostics |

Rooting stress uses `PERRY_GC_SCHEDULE_SEED`,
`PERRY_GC_SCHEDULE_RATE`, `PERRY_GC_SCHEDULE_ALLOC_KB`,
`PERRY_GC_FORCE_EVACUATE`, `PERRY_GC_VERIFY_EVACUATION`,
`PERRY_GC_PROTECT_FROMSPACE`, `PERRY_GC_PROTECT_FROMSPACE_DEPTH`,
`PERRY_GC_FROMSPACE_SCAN`, and `PERRY_GC_FROMSPACE_SCAN_ABORT`. Their exact
contracts and non-vacuity requirements live in the
[rooting invariant](gc-rooting-invariant.md). Research/bisection controls such
as `PERRY_GC_INCREMENTAL`, `PERRY_GC_PROMOTE_IN_PLACE`,
`PERRY_GC_MAJOR_PACING_FLOOR_MB`, `PERRY_GC_MAJOR_PACING_GROWTH`,
`PERRY_GC_MOVING_SAFEPOINT`, `PERRY_GC_MOVING_LOOP_POLLS`,
`PERRY_GC_SAFEPOINT_ONLY`, and `PERRY_STACKMAP_WALKER` are accepted but are not
additional supported collector modes.

`scripts/check_gc_env_knobs.py` derives the accepted names from live
runtime/codegen/compiler parsers and rejects a current document, executable
script, workflow, or translation catalog that names a deleted knob.

## Validation and CI authority

As of 2026-08-11, branch protection requires `lint`, `cargo-test`, `parity`,
`compile-smoke`, `api-docs-drift`, `security-audit`, and
`conformance-smoke-complete`. The GC-specific coverage is split deliberately:

| check | where it runs | required status |
|---|---|---|
| root-holder custody and GC-knob drift self-tests/live scans | `test.yml` → `lint` | yes (`lint`) |
| runtime unit suite and `run_memory_stability_tests.sh` four-mode matrix | `test.yml` → `cargo-test` | yes (`cargo-test`) |
| emitted root dominance, including native statepoint IR | `gc-root-dominance.yml` | not currently branch-required |
| pinned collector counters/RSS/wall matrix | `gc-ratchet.yml` | not currently branch-required |
| thread-local mechanism/policy budget | `tls-budget.yml` | not currently branch-required |

Useful local preflight commands:

```bash
python3 scripts/check_gc_env_knobs.py --self-test
python3 scripts/check_gc_env_knobs.py
python3 scripts/gc_runtime_root_holders.py --self-test
python3 scripts/gc_runtime_root_holders.py
python3 scripts/gc_root_dominance_check.py --self-test
cargo test -p perry-runtime --lib
```

The dedicated performance/rooting workflows have broader compiler and host
requirements; their workflow files are the authority for exact commands and
relevance filters.

## Historical evidence

- [Generational GC plan][generational-plan]: original design and
  phase-by-phase landing log.
- [Statepoint GC experiment][statepoint-experiment]: chronological
  prototype measurements and corrections leading to the native-root default.
- [GC rooting invariant](gc-rooting-invariant.md): current codegen soundness
  rule, checker modes, known blind spots, and debugging instruments.
- [Memory Model](memory-model.md): NaN-boxing, allocation representation, and
  platform memory-tooling notes.

[generational-plan]: https://github.com/PerryTS/perry/blob/main/docs/generational-gc-plan.md
[statepoint-experiment]: https://github.com/PerryTS/perry/blob/main/docs/statepoint-gc-experiment.md

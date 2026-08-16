# Memory Model

Perry compiles TypeScript directly to native code via LLVM, but JavaScript is a managed language: closures escape, objects outlive scopes, cycles exist. This page explains how Perry reconciles "native binary" with "garbage-collected language" — the value representation, the heap layout, how the GC finds roots, and how LLVM-generated code cooperates with the collector.

If you've ever wondered "does Perry use reference counting?" — no. There is no `Rc` at runtime. Perry has a real tracing GC, described below.

For the dated source of truth on shipped collection paths, target-specific
root lowerings, memory pressure, block pooling, supported knobs, and CI, see
[Garbage collector: current architecture and operations](garbage-collector.md).

## Value representation: NaN-boxing

Every JavaScript value in Perry is a single 64-bit word. The encoding piggy-backs on IEEE 754: any `f64` whose exponent is all-ones and whose mantissa is non-zero is a NaN, and there are ~2⁵² distinct NaN bit patterns. Perry uses the high 16 bits as a type tag and the low 48 (or 32) bits as the payload.

| Tag (high 16 bits) | Type | Payload |
|---|---|---|
| `0x7FFC…0001` | `undefined` | — (singleton) |
| `0x7FFC…0002` | `null` | — (singleton) |
| `0x7FFC…0003` | `false` | — (singleton) |
| `0x7FFC…0004` | `true` | — (singleton) |
| `0x7FF9` | Short string (0–5 bytes) | length in bits 40–47, the bytes in bits 0–39 — no allocation |
| `0x7FFA` | BigInt | low 48 bits = heap pointer |
| `0x7FFB` | JS handle | low 48 bits = handle id (V8-backed objects) |
| `0x7FFD` | Object / Array / Closure | low 48 bits = heap pointer |
| `0x7FFE` | Int32 | low 32 bits = signed int |
| `0x7FFF` | String | low 48 bits = heap pointer |
| anything else | `f64` | the full 64 bits are the number |

<!-- gc-fact: POINTER_TAG = 0x7FFD_0000_0000_0000 in crates/perry-runtime/src/value/tags.rs -->
<!-- gc-fact: STRING_TAG = 0x7FFF_0000_0000_0000 in crates/perry-runtime/src/value/tags.rs -->
<!-- gc-fact: INT32_TAG = 0x7FFE_0000_0000_0000 in crates/perry-runtime/src/value/tags.rs -->
<!-- gc-fact: BIGINT_TAG = 0x7FFA_0000_0000_0000 in crates/perry-runtime/src/value/tags.rs -->
<!-- gc-fact: SHORT_STRING_TAG = 0x7FF9_0000_0000_0000 in crates/perry-runtime/src/value/tags.rs -->
<!-- gc-fact: TAG_UNDEFINED = 0x7FFC_0000_0000_0001 in crates/perry-runtime/src/value/tags.rs -->

Source: `crates/perry-runtime/src/value/tags.rs` (the singleton and pointer tags),
with the rest of the `value/` module tree for the encode/decode helpers.
`TAG_HOLE` and `TAG_TDZ` are two further `0x7FFC` singletons the runtime uses
internally; they are not user-observable values.

Three consequences worth noting:

1. **Numbers are free.** A plain `f64` value is its own representation — no boxing, no header, no allocation. Numeric hot loops cost nothing in memory traffic.
2. **The GC can identify pointer values from the tag alone.** When tracing a value, the collector masks the high bits, checks for `0x7FFA`/`0x7FFD`/`0x7FFF`, and either follows the low-48-bit pointer or skips. There is no per-value runtime type lookup.
3. **Type checks are bitwise.** `typeof` and many fast paths in the runtime are register-level mask-and-compare operations.

## Heap layout: per-thread arena, nursery + old-gen

Perry is single-threaded by default, and each thread owns its own heap. Sharing across threads happens via deep copy (`SerializedValue`), not shared memory, so the GC never has to synchronize across threads.

Within a thread, the heap is two arenas:

- **`ARENA`** — the nursery. New allocations land here. Carved into 1 MB blocks (since v0.5.196).
- **`OLD_ARENA`** — the old generation. Holds objects that have survived enough minor GCs to be tenured.

Every allocation, in either arena, is prefixed by an 8-byte `GcHeader`
(`crates/perry-runtime/src/gc/types.rs`):

```rust
#[repr(C)]
pub struct GcHeader {
    pub obj_type: u8,    // GC_TYPE_ARRAY, GC_TYPE_STRING, …
    pub gc_flags: u8,    // MARKED | ARENA | PINNED | TENURED | HAS_SURVIVED | …
    pub _reserved: u16,
    pub size: u32,       // total alloc size, used for arena block walking
}
```

Callers receive a pointer **after** the header (`ptr + 8`), so from TypeScript code's perspective the header is invisible. The collector finds the header by subtracting 8.

Allocation goes through `gc_malloc(size, obj_type)` in the `gc/` module tree.
LLVM-generated code emits calls to this for every object literal, array
literal, closure capture, string concat, BigInt operation, etc. Going through
the GC allocation funnel is how the collector accounts for memory and decides
when to collect.

## How the GC finds roots

This is the part most people are surprised by: if Perry compiles through LLVM, the optimizer is free to keep values in registers, spill them to stack slots, rematerialize them — none of which the collector can introspect. So how does the collector know which JS values are live?

Three mechanisms cover different storage locations:

### 1. Target-aware precise roots (codegen-emitted)

One pointer-local analysis feeds two correct lowerings. On supported 64-bit
AArch64/arm64 and x86-64 targets, native RS4GC statepoints plus Perry's compact
stack map are the default. `arm64_32` watchOS, ARM64 Windows, and unsupported
architectures use Perry's heap-backed shadow frames. The fallback is a root
lowering, not an unrooted mode; see [the current GC page](garbage-collector.md#roots-by-target).

At a safepoint the selected map describes each live managed local regardless of
whether LLVM kept it in a register, spilled it, or relocated it.

### 2. Conservative native-stack diagnostic

The production default does not scan the native stack conservatively: `Auto`
resolves to `SkipDisabled`. `PERRY_CONSERVATIVE_STACK_SCAN=full` is a diagnostic
sensitivity arm that scans words which look like arena pointers and pins the
corresponding objects for that cycle. Because an ambiguous root cannot be
rewritten safely, this arm makes the copying minor ineligible.

### 3. Registered runtime root scanners

Some roots live in the runtime itself, not in user code: pending Promises,
timer callbacks, exception state, async-context stacks, shape caches, overflow
fields, parse scratch tables, and intern tables. The collector invokes the
registered scanners during marking; `scripts/gc_runtime_root_holders.py`
enumerates the holders and refuses unclassified or stale inventory entries.

## Generational behaviour

Most JS allocations die young — object literals in a loop body, short-lived closures, intermediate strings. A generational collector exploits this by collecting the nursery frequently and the old gen rarely.

Aging is recorded two different ways, because the two minor paths need different
things from it.

**The non-copying minor** uses two flag bits in `gc_flags`
(`crates/perry-runtime/src/gc/types.rs`): `GC_FLAG_HAS_SURVIVED` on the first
minor an object survives, `GC_FLAG_TENURED` on the second, at which point the
object is logically promoted. The two-bit scheme avoids a counter field in the
header, and a tenured object initially stays physically where it is — that
promotion is a flag flip, not a copy. Whether such objects are then evacuated
into `OLD_ARENA` (with every reference rewritten) is a policy decision taken per
cycle from nursery/RSS pressure and measured movable candidates, and it requires
generated write barriers to be active.

**The copying minor** — the default nursery path — stores an exact short age in
the header instead, and its promotion threshold is *adaptive*: a
survivor-occupancy feedback loop picks the largest number of survivals whose projected
survivor occupancy still fits, capped at 4, and locks to 1 (promote on first
copy) when the aging round is measurably filtering nothing. There is no fixed
`PROMOTION_AGE` and no knob; `crates/perry-runtime/src/gc/tenuring.rs` is the
definition.

When a copying minor's whole young generation measures (near-)entirely live it
does not copy at all: the blocks change generation in place. The current GC page
covers that path, its thresholds, and its footprint budgets —
[Aging, promotion, and where an object is born](garbage-collector.md#aging-promotion-and-where-an-object-is-born).

### Write barriers and the remembered set

Generational collectors have one fundamental problem: if an old-gen object points to a young-gen object, a minor GC (which only traces the nursery) needs to know about that pointer or it will free a live object.

The fix is a **write barrier**: every time a pointer field is written, the runtime checks "is this old → young?" and, if so, records the parent in a **remembered set**. Minor GCs treat remembered-set entries as additional roots.

In Perry, codegen emits write-barrier calls by default so copied minor GC and
evacuation can rely on exact remembered-set data. Set
`PERRY_WRITE_BARRIERS=0`/`off`/`false` during compile for bisection; at runtime,
the same setting disables exact helper barriers. Generational minors then fall
back to full mark-sweep rather than trusting an empty remembered set.

## Triggers and tuning

`gc_check_trigger` in `gc/policy.rs` responds to four signal families:

1. **Nursery pressure** — allocation growth and the adaptive nursery cap.
2. **Malloc count pressure** — too many separately tracked allocations.
3. **Major pacing** — unreclaimed post-collection bytes outgrow the live baseline.
4. **Explicit/host requests** — user `gc()` and warning/critical OS memory pressure.

Device/container budgets scale trigger and reclaim ceilings down. Released
blocks are reused thread-locally under a process-wide byte cap before allocator
return; see the current GC page for the pressure levels and the pool's cap and
drain behaviour.

## Escape hatches and diagnostics

| Env var | Effect |
|---|---|
| `PERRY_GEN_GC=0` / `off` / `false` | Disable generational mode; fall back to full mark-sweep (intended for bisection only). |
| `PERRY_GC_FORCE_EVACUATE=1` | With generated write barriers active and policy evacuation allowed, stress-copy every marked non-pinned nursery object instead of only tenured survivors. |
| `PERRY_GC_VERIFY_EVACUATION=1` | After an evacuation that actually forwards objects, panic if any mutable live slot still points at a forwarded nursery object after rewrite. |
| `PERRY_WRITE_BARRIERS=0` / `off` / `false` | Disable codegen-emitted write barriers at compile time and runtime exact helper barriers at runtime for benchmark/debug bisection. Unset, `=1`, `=on`, and `=true` keep barriers enabled. |
| `PERRY_GC_DIAG=1` | Print per-cycle diagnostics, including one evacuation-policy line for cycles where evacuation was considered and for `barriers_inactive` skips. |

## Rooting-bug instruments

A value that is live but not rooted across a collection point leaves nothing
behind at collection time — there is literally nothing for the collector to
find. The nursery then recycles the address immediately, so the stale pointer
reads a valid unrelated object and the program dies a cycle or more later, in a
different function, as `TypeError: value is not a function`. These knobs exist
to collapse that detection latency. **All are default-off and inert when off.**

| Env var | Effect |
|---|---|
| `PERRY_GC_PROTECT_FROMSPACE=1` | After an **evacuating (copying) minor**, do not recycle from-space. Retired Eden and active-survivor blocks are detached into a bounded quarantine, filled with a poison pattern whose first byte reads as an invalid `obj_type` (`0xDE`), and `mprotect(PROT_NONE)`'d over their page-aligned interior. A stale dereference then SIGSEGVs **at the faulting instruction**, with the holder still on the stack. The installed reporter prints the faulting address, which minor retired it, and the last-known object that lived there (`obj_type`, size) plus a native backtrace, then restores `SIG_DFL` and returns so the instruction re-faults — a core file or debugger still sees the real crash site. |
| `PERRY_GC_PROTECT_FROMSPACE=poison` | As above without `mprotect`: poison only. Use where a fault is unwanted, or for the sub-page block edges `mprotect` cannot cover (those are always poison-filled and counted separately). |
| `PERRY_GC_PROTECT_FROMSPACE_DEPTH=N` | How many retired page-sets stay quarantined (default `4`, minimum `1`). Expired sets are restored to read/write and **recycled back into Eden**, never freed, so the quarantine is a ring: steady-state footprint is bounded by `N × from-space bytes` and no `mprotect`'d page is ever handed to the system allocator. |
| `PERRY_GC_FROMSPACE_SCAN_ABORT=1` | Abort on the **first** offending slot the whole-heap from-space scan finds, printing slot, holder, target (including the target's `obj_type`) and a collector backtrace. Now implies `PERRY_GC_FROMSPACE_SCAN=1`; previously it was silently inert on its own. |
| `PERRY_GC_SCHEDULE_SEED=<u64>` | **Seeded GC-schedule fuzzing** — when nursery pressure is not due, add a minor collection at a handled safepoint when a deterministic pseudo-random function of the seed and a per-thread safepoint ordinal selects it. It never *suppresses* a pressure-driven collection; the rate is additional density on top of normal pacing. A failing seed is a reproducer. The schedule implies forced evacuation unconditionally; #7611 deleted the ambient evacuation veto that could silently disarm this instrument. It does **not** bypass `gc_safepoint_moving_minor`'s entry guards (in-allocation, suppressed, unsafe FFI zone, non-zero root-lock depth, budgeted cycle): a safepoint reached in any of those states still declines to collect, and does not consume a schedule slot. A value that does not parse as a `u64` reads as OFF, not as seed 0. Composes with the two above; that pairing is what turns a rooting bug into an immediate precise fault. |
| `PERRY_GC_SCHEDULE_RATE=<0..1>` | Expected fraction of eligible handled safepoints that receive an *additional* schedule-triggered collection (default `0.05`). Inert without a seed. `0` selects nothing but still installs the banner and reporters, so it is a clean control arm; `1` collects at every handled safepoint — maximum pressure, in the spirit of V8's `--stress-scavenge`, and the point where the seed stops mattering because every ordinal is selected. Out-of-range values clamp. |

These instruments have explicit caveats, because each has burned a prior
investigation:

- `PERRY_GC_PROTECT_FROMSPACE` gates **only** the copying minor's from-space
  reset. A run with the knob on and zero copying minors protects nothing. Check
  for a `[gc-fromspace-protect] retired_set=#N` line under `PERRY_GC_DIAG=1`.
- **Depth is the knob to raise when a suspected bug does not fault.** A stale
  pointer is only caught while the page-set it names is still quarantined, and
  at `PERRY_GC_SCHEDULE_SEED=<u64> PERRY_GC_SCHEDULE_RATE=1
  PERRY_GC_SCHEDULE_ALLOC_KB=0` a value can cross hundreds of collections between
  its last valid observation and its stale use — one per loop back-edge poll.
  (`ALLOC_KB=0` is what makes that literally per-poll: rate 1 selects every
  *candidate*, and the default 4 KB stride only makes a poll a candidate once
  that much new nursery material has accumulated.) On #7154's
  `new C(…)` reproducer the constructor body runs 600 polls, so the caller's
  stale register is 600 retirements old by the time the return-override
  publishes it: the default depth of 4 misses it silently, and
  `PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` faults on the first use. Rule of thumb:
  depth ≥ the number of safepoints the suspect value survives.
- `PERRY_GC_SCHEDULE_SEED` cannot select loop back-edge polls that codegen never
  produced. Those are a **compile-time** property (`PERRY_GC_MOVING_LOOP_POLLS`, default
  ON since #7721; a binary compiled with `=0` has none). Without them, a seeded
  run only fires at event-loop boundaries and a compute-only loop never
  collects at all — check the exit summary's `loop_polls=` before trusting a
  clean sweep. At `PERRY_GC_SCHEDULE_RATE=1` you no longer have to remember:
  the run prints a `[gc-schedule]` verdict at exit and **exits 70** when it
  forced or moved nothing, so a vacuous run is a red run rather than a green
  one. Sub-endpoint rates get the summary line but no hard verdict (a sparse
  seed legitimately forcing nothing is not a broken instrument).
- **`PERRY_GC_SCHEDULE_SEED`'s determinism is per-thread, and that is the honest
  scope.** The safepoint counter is thread-local: no wall clock, no address, no
  thread identity enters the decision, so a **single-threaded** program replays
  a seed exactly. A `perry/thread` program gets a deterministic schedule *per
  thread given that thread's own safepoint sequence*, but nothing makes the OS
  schedule that sequence identically twice, so a multi-threaded reproducer is
  only as reproducible as its threading. A global counter would be strictly
  worse — it would make even a single thread's schedule depend on interleaving.
  Report which case you measured.
- **A clean sweep means nothing without a safepoint count.** The mode prints
  `[gc-schedule] done: seed=… safepoints=… scheduled_collections=…` at exit, and
  `scripts/gc_schedule_fuzz.sh` refuses to call a sweep clean when every run
  reported zero safepoints — the usual cause being a binary compiled without
  `PERRY_GC_MOVING_LOOP_POLLS=1`, which has no in-loop safepoints for a schedule
  to select.
- **Page protection is Unix-only.** `mprotect` / `sigaction` / `sysconf` are not
  exposed by the `libc` crate on `x86_64-pc-windows-msvc`, a target
  `perry-runtime` is genuinely built for. On non-Unix hosts `=1` degrades to
  `poison`, which is visible rather than silent: `bytes_protected` stays `0`
  while `bytes_poisoned` counts the whole retired range.

## Why this design

The combination—NaN-boxing, per-thread arenas, target-aware precise roots,
registered runtime scanners, barriers, and generational aging—is what lets
Perry go through LLVM and still run a moving managed heap.

Going to native code does not preclude having a GC. It means the collector's
relationship with compiled code is mediated by an ABI and the selected root
map; the linked runtime remains a real tracing collector. There is nothing
reference-counted at runtime.

## Profiling Perry's memory on macOS

Two things to know before reading `vmmap`, Instruments' VM Tracker, or
`footprint` output for a Perry binary:

- **The heap does not show up under `MALLOC_*`.** Perry routes all runtime
  allocation through mimalloc (`#[global_allocator]`, issue #62), whose
  mappings appear as their own anonymous regions, not in the system malloc
  zones.
- **Those regions used to render as `IOAccelerator`** — i.e. GPU driver
  memory — because mimalloc tags its mappings with VM tag 100, which macOS
  tooling decodes as `IOAccelerator`. The runtime retags them to
  `VM_MEMORY_APPLICATION_SPECIFIC_1` (240), so the JS heap shows up as
  **`Memory Tag 240`** and there is no GPU memory involved. Set
  `MIMALLOC_OS_TAG=<n>` to steer the tag yourself — the runtime defers to an
  explicit env setting.

  The retag runs from a `__DATA,__mod_init_func` constructor
  (`perry-runtime/src/mimalloc_os_tag.rs`), *not* from `js_gc_init`, and that
  placement is load-bearing: mimalloc reserves a 1 GiB arena on its first
  allocation — during `std`'s pre-`main` startup — and every later allocation
  just commits pages inside that already-tagged region, so setting the option
  from any Rust code, `main` included, retags nothing that matters. #6882 set
  it from `js_gc_init` and was therefore inert for the whole heap until #7450
  moved it pre-`main`. On a build between those two, expect ~all of the heap
  as `IOAccelerator` and a token `Memory Tag 240` region.

  So: large `IOAccelerator` regions on a current build are a *bug*, not a
  documentation caveat — most likely the module initializer was dropped from
  the link. `crates/perry-runtime/src/gc/tests/os_tag.rs` asserts the real
  thing (the kernel's `user_tag` for a live heap address, via
  `mach_vm_region`); note that `mi_option_get(mi_option_os_tag)` reports 240
  in the broken case too, so it is not a diagnosis.

Also note that mimalloc purges freed memory with `MADV_FREE`-style advice:
macOS keeps such pages counted in RSS and `phys_footprint` until memory
pressure, so headline RSS numbers overstate what the process would actually
hold onto under pressure.

## Source map

Paths, not line numbers: a line number is a claim nothing re-derives, and every
row of this table once pointed into a `gc.rs` that no longer exists.

| Topic | File | Symbol to look for |
|---|---|---|
| NaN-boxing tags | `crates/perry-runtime/src/value/tags.rs` | `POINTER_TAG`, `STRING_TAG` |
| `GcHeader`, type/flag constants | `crates/perry-runtime/src/gc/types.rs` | `GcHeader` |
| `gc_malloc` | `crates/perry-runtime/src/gc/malloc.rs` | `gc_malloc` |
| Shadow frames (fallback root lowering) | `crates/perry-runtime/src/gc/roots/shadow_stack.rs` | `js_shadow_frame_push` |
| Registered root scanners | `crates/perry-runtime/src/gc/roots.rs` | `gc_register_mutable_root_scanner` |
| Copying minor | `crates/perry-runtime/src/gc/copying.rs` | `move_young` |
| Whole-block in-place promotion | `crates/perry-runtime/src/arena/promote.rs` | `finish_in_place_promotion` |
| Promotion policy | `crates/perry-runtime/src/gc/promote_in_place.rs` | `PROMOTE_SURVIVAL_THRESHOLD_PERMILLE` |
| Adaptive tenuring threshold | `crates/perry-runtime/src/gc/tenuring.rs` | `tenuring_survivals` |
| Write barriers | `crates/perry-runtime/src/gc/barrier_store.rs` | `runtime_write_barrier_slot` |
| Explicit pinning | `crates/perry-runtime/src/gc/pin.rs` | `pin_object` |
| Conservative-pin predicate | `crates/perry-runtime/src/gc/verify.rs` | `is_conservatively_pinned` |
| Collection triggers and pacing | `crates/perry-runtime/src/gc/policy.rs` | `gc_check_trigger` |
| Current operations page | `docs/src/internals/garbage-collector.md` | — |
| Design plan (historical) | `docs/generational-gc-plan.md` | — |

<!-- gc-symbol: GcHeader in crates/perry-runtime/src/gc/types.rs -->
<!-- gc-symbol: gc_malloc in crates/perry-runtime/src/gc/malloc.rs -->
<!-- gc-symbol: js_shadow_frame_push in crates/perry-runtime/src/gc/roots/shadow_stack.rs -->
<!-- gc-symbol: gc_register_mutable_root_scanner in crates/perry-runtime/src/gc/roots.rs -->
<!-- gc-symbol: move_young in crates/perry-runtime/src/gc/copying.rs -->
<!-- gc-symbol: finish_in_place_promotion in crates/perry-runtime/src/arena/promote.rs -->
<!-- gc-symbol: tenuring_survivals in crates/perry-runtime/src/gc/tenuring.rs -->
<!-- gc-symbol: runtime_write_barrier_slot in crates/perry-runtime/src/gc/barrier_store.rs -->
<!-- gc-symbol: pin_object in crates/perry-runtime/src/gc/pin.rs -->
<!-- gc-symbol: is_conservatively_pinned in crates/perry-runtime/src/gc/verify.rs -->
<!-- gc-symbol: gc_check_trigger in crates/perry-runtime/src/gc/policy.rs -->

Those rows are not decorative: each is bound to a `gc-symbol` marker above and
re-derived by `scripts/check_gc_doc_claims.py`, so a rename that leaves this
table behind fails `lint` instead of quietly pointing readers at nothing.

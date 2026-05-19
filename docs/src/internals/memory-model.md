# Memory Model

Perry compiles TypeScript directly to native code via LLVM, but JavaScript is a managed language: closures escape, objects outlive scopes, cycles exist. This page explains how Perry reconciles "native binary" with "garbage-collected language" — the value representation, the heap layout, how the GC finds roots, and how LLVM-generated code cooperates with the collector.

If you've ever wondered "does Perry use reference counting?" — no. There is no `Rc` at runtime. Perry has a real tracing GC, described below.

## Value representation: NaN-boxing

Every JavaScript value in Perry is a single 64-bit word. The encoding piggy-backs on IEEE 754: any `f64` whose exponent is all-ones and whose mantissa is non-zero is a NaN, and there are ~2⁵² distinct NaN bit patterns. Perry uses the high 16 bits as a type tag and the low 48 (or 32) bits as the payload.

| Tag (high 16 bits) | Type | Payload |
|---|---|---|
| `0x7FFC…0001` | `undefined` | — (singleton) |
| `0x7FFC…0002` | `null` | — (singleton) |
| `0x7FFC…0003` | `false` | — (singleton) |
| `0x7FFC…0004` | `true` | — (singleton) |
| `0x7FFA` | BigInt | low 48 bits = heap pointer |
| `0x7FFD` | Object / Array / Closure | low 48 bits = heap pointer |
| `0x7FFE` | Int32 | low 32 bits = signed int |
| `0x7FFF` | String | low 48 bits = heap pointer |
| anything else | `f64` | the full 64 bits are the number |

Source: `crates/perry-runtime/src/value.rs`.

Three consequences worth noting:

1. **Numbers are free.** A plain `f64` value is its own representation — no boxing, no header, no allocation. Numeric hot loops cost nothing in memory traffic.
2. **The GC can identify pointer values from the tag alone.** When tracing a value, the collector masks the high bits, checks for `0x7FFA`/`0x7FFD`/`0x7FFF`, and either follows the low-48-bit pointer or skips. There is no per-value runtime type lookup.
3. **Type checks are bitwise.** `typeof` and many fast paths in the runtime are register-level mask-and-compare operations.

## Heap layout: per-thread arena, nursery + old-gen

Perry is single-threaded by default, and each thread owns its own heap. Sharing across threads happens via deep copy (`SerializedValue`), not shared memory, so the GC never has to synchronize across threads.

Within a thread, the heap is two arenas:

- **`ARENA`** — the nursery. New allocations land here. Carved into 1 MB blocks (since v0.5.196).
- **`OLD_ARENA`** — the old generation. Holds objects that have survived enough minor GCs to be tenured.

Every allocation, in either arena, is prefixed by an 8-byte `GcHeader` (`crates/perry-runtime/src/gc.rs:14`):

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

Allocation goes through `gc_malloc(size, obj_type)` (`gc.rs:606`). LLVM-generated code emits calls to this for every object literal, array literal, closure capture, string concat, BigInt operation, etc. There is no allocation primitive in the IR that bypasses this — going through `gc_malloc` is how the GC accounts for live memory and decides when to collect.

## How the GC finds roots

This is the part most people are surprised by: if Perry compiles through LLVM, the optimizer is free to keep values in registers, spill them to stack slots, rematerialize them — none of which the collector can introspect. So how does the collector know which JS values are live?

Three mechanisms, used together:

### 1. Precise shadow stack (codegen-emitted)

Codegen emits, at function entry, a call to `js_shadow_frame_push(slot_count)` (`gc.rs:493`). This reserves a frame in a thread-local shadow stack. Every JS-level local variable in the function gets a slot, and every assignment to that local emits a paired `js_shadow_slot_set(idx, value)` call. On function exit, codegen emits `js_shadow_frame_pop`.

The result: at any GC safepoint, the collector can walk the shadow stack and see the live NaN-boxed value of every TS-level local in every active frame, regardless of what LLVM did with registers. This is the "precise" half of the root scan — `shadow_stack_root_scanner` (`gc.rs:3860`).

### 2. Conservative native-stack scan

Some values are not on the shadow stack — most importantly, anything currently in a CPU register or in a Rust runtime frame at the moment GC fires. For these, the collector scans the native stack word-by-word and, for each word, checks whether it looks like a pointer into one of the arenas. Anything that does is **conservatively pinned** for that cycle (`is_conservatively_pinned`, `gc.rs:3747`).

Pinning means: the object isn't freed, and isn't moved (the evacuation pass skips it). False positives are acceptable — they just keep a dead object alive for one more cycle. False negatives would be catastrophic — they'd free a live object — and the shadow stack + scanner registration below ensure they don't happen for known roots.

### 3. Registered runtime root scanners

Some roots live in the runtime itself, not in user code: pending Promises, timer callbacks, exception state, async-context stacks, async-hooks state, shape caches, transition caches, overflow fields, JSON-parse scratch tables, the string intern table. Each is registered with the collector via `gc_register_root_scanner(scanner_fn)` (`gc.rs:807`), and the collector invokes each scanner during the mark phase. There are 9 such scanners currently registered (`gc.rs:3232`–`3940`).

## Generational behaviour

Most JS allocations die young — object literals in a loop body, short-lived closures, intermediate strings. A generational collector exploits this by collecting the nursery frequently and the old gen rarely.

Perry uses two-bit aging encoded in `gc_flags` (`gc.rs:64`):

- First minor GC an object survives: `GC_FLAG_HAS_SURVIVED` is set.
- Second minor GC it survives: `GC_FLAG_TENURED` is set, and the object is logically promoted to old-gen.

`PROMOTION_AGE = 2`. The two-bit scheme avoids needing a counter field in the header.

Tenured objects initially stay physically where they are in the nursery — promotion is a flag flip, not a copy. A telemetry-driven **evacuation policy** copies tenured non-pinned objects into `OLD_ARENA` and rewrites all references to point at the new locations only when generated write barriers are active and nursery/RSS pressure plus measured movable candidates justify the extra work. Low-pressure cycles, cycles with no movable candidates, and cycles without generated write barriers skip evacuation and reference rewriting.

### Write barriers and the remembered set

Generational collectors have one fundamental problem: if an old-gen object points to a young-gen object, a minor GC (which only traces the nursery) needs to know about that pointer or it will free a live object.

The fix is a **write barrier**: every time a pointer field is written, the runtime checks "is this old → young?" and, if so, records the parent in a **remembered set**. Minor GCs treat remembered-set entries as additional roots.

In Perry, the runtime barrier is always present: `js_write_barrier(parent, child)` (`gc.rs:3773`). Codegen emits write-barrier calls by default so copied minor GC and evacuation can rely on exact dirty-page data. Set `PERRY_WRITE_BARRIERS=0`/`off`/`false` during compile to suppress generated barrier calls for benchmark/debug bisection; at runtime, the same setting disables runtime exact helper barriers. Copied-minor and evacuation then treat barrier data as inactive and fall back to conservative paths.

## Triggers and tuning

`gc_check_trigger` (`gc.rs:919`) fires on three signals:

1. **Arena block allocation** — every time a new 1 MB block is allocated for the nursery.
2. **Malloc count threshold** — too many malloc-tracked objects (strings, closures, …) outstanding.
3. **Explicit `gc()` call** from user code.

The next-trigger calculation steps up after each cycle but is hard-capped at the initial threshold (64 MB) so that a workload which frees >90% of the nursery on each cycle can't drift peak occupancy upward through step-doubling (C4b-δ-tune, v0.5.236).

Idle nursery blocks observed empty for 2 GC cycles are `dealloc`'d back to the OS (C4b-δ, v0.5.235), so a workload's RSS shrinks once the burst is over.

## Escape hatches and diagnostics

| Env var | Effect |
|---|---|
| `PERRY_GEN_GC=0` / `off` / `false` | Disable generational mode; fall back to full mark-sweep (intended for bisection only). |
| `PERRY_GEN_GC_EVACUATE=0` / `off` / `false` | Disable policy evacuation. `=1` / `on` / `true` is accepted as "allow the auto-policy", not as unconditional evacuation. |
| `PERRY_GC_FORCE_EVACUATE=1` | With generated write barriers active and policy evacuation allowed, stress-copy every marked non-pinned nursery object instead of only tenured survivors. |
| `PERRY_GC_VERIFY_EVACUATION=1` | After an evacuation that actually forwards objects, panic if any mutable live slot still points at a forwarded nursery object after rewrite. |
| `PERRY_WRITE_BARRIERS=0` / `off` / `false` | Disable codegen-emitted write barriers at compile time and runtime exact helper barriers at runtime for benchmark/debug bisection. Unset, `=1`, `=on`, and `=true` keep barriers enabled. |
| `PERRY_GC_DIAG=1` | Print per-cycle diagnostics, including one evacuation-policy line for cycles where evacuation was considered and for `barriers_inactive` skips. |

## Why this design

The combination — NaN-boxing for cheap value representation, per-thread arenas to avoid cross-thread sync, precise shadow stack + conservative stack scan for safe root discovery under an opaque optimizer (LLVM), generational aging for nursery-friendly workloads — is what lets Perry both go through LLVM and run a managed language without a fight.

Going to native code does not preclude having a GC. It just means the GC's relationship with the compiled code is mediated by an ABI: codegen emits calls to `gc_malloc`, `js_shadow_frame_push/pop`/`js_shadow_slot_set`, and `js_write_barrier`, and the runtime crate (linked in as native code) is a real generational mark-sweep collector. There is nothing reference-counted at runtime.

## Source map

| Topic | File |
|---|---|
| NaN-boxing constants and helpers | `crates/perry-runtime/src/value.rs` |
| `GcHeader`, type/flag constants | `crates/perry-runtime/src/gc.rs:14` |
| `gc_malloc` | `crates/perry-runtime/src/gc.rs:606` |
| Shadow stack | `crates/perry-runtime/src/gc.rs:493`–`583` |
| Minor GC | `crates/perry-runtime/src/gc.rs:1192` |
| Write barrier | `crates/perry-runtime/src/gc.rs:3773` |
| Registered root scanners | `crates/perry-runtime/src/gc.rs:3232`–`3940` |
| Conservative pin set | `crates/perry-runtime/src/gc.rs:3747` |
| Design plan (pre-implementation) | `docs/generational-gc-plan.md` |

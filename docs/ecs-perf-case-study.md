# Closing the perry/bun ratio on ECS workloads

**Status:** complete. Written 2026-05-03 after landing the final commit
on `wip/ecs-loop-fixes`.
**Goal:** every workload in [`@codehz/ecs`](https://github.com/codehz/ecs)
(via the `ecs-perf-test` test fixture) hits **perry/bun wall-time ratio
≤ 1.10×** with default GC settings, with zero changes to the vanilla
TypeScript ECS code itself.
**Outcome:** all 6 workloads under 1.10×; the previously-stuck
`perf-comprehensive` bench landed at **1.04× of bun** after a chain of
9 commits.

This is the record of how that ratio came down. The intent is twofold:
to capture each lever that worked (and the ones that didn't, with
reasoning) so future perf work doesn't re-tread these paths, and to
document the profiling workflow that surfaced the bottlenecks — most
of which weren't where reading the code would suggest.

## Final standings

| workload | bun | perry | ratio | source |
|---|---:|---:|---:|---|
| world-perf | 34 | 19 | 0.55× | `src/__tests__/world-perf.test.ts` |
| query-perf | 37 | 19 | 0.51× | `src/__tests__/query-perf.test.ts` |
| demo-simple | 29 | 16 | 0.56× | `examples/simple/demo.ts` |
| demo-advanced | 28 | 17 | 0.62× | `examples/advanced-scheduling/demo.ts` |
| sync-hotpath | 144 | 51 | 0.36× | `src/__tests__/sync-hotpath.perf.test.ts` |
| **perf-comprehensive** | **396** | **412** | **1.04×** | `src/__tests__/perf-comprehensive.perf.test.ts` |

`perf-comprehensive` was the holdout. At session start it sat at 1.51×
of bun (~620 ms vs ~410 ms); the previous iteration's GC-trigger band
widening (`fb215750`) had brought it down from 1.94×, and earlier
cross-module inlining work (`4366ab95`, `26a6b614`, `2b104b3d`) had
established the inlining baseline. Everything described below is
incremental on top of that.

## Constraints (load-bearing)

- **No changes to ECS code.** All 6 workloads compile from the vanilla
  TypeScript `@codehz/ecs` — the test fixture lives at
  `/Users/amlug/projects/perry/ecs-perf-test`. If perry can't make the
  vanilla code fast, the perf bar isn't met. A handful of attempts to
  rewrite hot ECS paths got reverted; perf has to come from the
  compiler and runtime.
- **Default GC settings.** No `PERRY_GEN_GC=0` or `PERRY_DEBUG_SYMBOLS=1`
  in the bar's measurement runs.
- **No regression on the 5 currently-passing workloads.** A change
  that pushed any of them past 1.10× of bun blocks the commit.

## Bench harness

```
./compare.sh <name> <src> bin/<name>
```

This runs bun + node + the perry-compiled binary back-to-back and
prints wall time. The fixture's `compare.sh` does a warmup run before
the measured one to absorb first-launch overhead.

For commits that touched `perry-runtime` or `perry-codegen`, the cycle
was: rebuild perry (`cargo build --release --bin perry`), recompile the
6 binaries (`perry compile -o bin/<name> <src>`), then bench each at
≥3 runs to absorb GC variance.

## How to see what's slow

Building `perry compile` with `PERRY_DEBUG_SYMBOLS=1` keeps symbols in
the linked binary. From there:

```
samply record --no-open --save-only -o /tmp/profile.json -r 4000 \
  --iteration-count 25 ./bin/perf-comprehensive-sym
```

samply emits a Firefox Profiler JSON. The resolved-name walk is in
`scripts/` of this iteration's bench machinery, but the gist is:

1. Parse the JSON's `stackTable`/`frameTable`/`funcTable` arrays.
2. For each sample's stack, walk the prefix chain to enumerate frames
   from leaf to root.
3. Aggregate **leaf** counts (where the CPU was actively executing) and
   **inclusive** counts (any frame on the stack).
4. The samply profile stores instruction offsets, not symbol names —
   resolve via `atos -o bin/perf-comprehensive-sym -l 0x100000000 <addr>`.

Two things this profiling workflow surfaced that reading the code
wouldn't have:

- A 4-instruction leaf function (`EntityIdManager.getNextId`) that
  showed up at 17-22% leaf samples across runs **didn't actually
  account for that time**. ARM64 frame-pointer omission for tiny
  leaves causes adjacent runtime functions' PCs to be attributed to
  the leaf. We confirmed this by checking the disassembly (4 LDR
  instructions, no stack frame) and the source — `getNextId` is only
  called from test files, never from the perf workloads.
- The dominant cost on `perf-comprehensive` after every other
  optimization was **HashMap SipHash** on thread-local registries
  (Map/Set/Buffer/typed-array address sets), not anything in user
  code or codegen.

## The chain of fixes

Listed in landing order on `wip/ecs-loop-fixes`. Each line: commit hash,
one-line summary, and the measured perf-comprehensive impact.

### `2a581fb7` perf(array.push): conditional writeback (–56 ms)

`CommandBuffer.set` does
```ts
this.commands.push({ type: "set", entityId, componentType, component });
```
~60 k times per round. The codegen `array.push` fast path was always
writing the (possibly-realloc'd) array pointer back to the receiver
via `js_object_set_field_by_name`. But `js_array_push_f64` only
returns a different pointer when capacity ran out — i.e. on a grow
event. With amortized doubling, real reallocs are O(log N) of the
total pushes; the other 99.9 % of pushes paid a no-op writeback that
still cost ~50-100 cycles inside `js_object_set_field_by_name`.

The fix is two extra LLVM instructions: save the pre-loop input
handle, after the push chain emit `icmp ne new_handle, orig_handle`,
and put the writeback in a cold block.

```rust
let orig_handle = arr_handle.clone();
for v in &lowered { /* ... js_array_push_f64 ... */ }
let blk = ctx.block();
let changed = blk.icmp_ne(I64, &new_handle, &orig_handle);
let wb_idx = ctx.new_block("arr.push.wb");
let merge_idx = ctx.new_block("arr.push.merge");
blk.cond_br(&changed, &wb_label, &merge_label);
// ... wb block does the js_object_set_field_by_name ...
```

Code: `crates/perry-codegen/src/lower_call/native.rs` — the
`if module == "array" && method == "push"` arm.

Result: perf-comprehensive 620 → 564 ms (–9 %, 1.51× → 1.43× of bun).
sync-hotpath 58 → 51 ms as a side benefit (same call site, less hot).

### `9990ee73` fix(ic): cache field slots ≥ 8 (correctness; perf-neutral here)

Diagnostic counters showed `js_object_get_field_ic_miss` was being
called ~900 k times per perf-comprehensive run. Investigating why,
I found this in the runtime's IC miss handler:

```rust
if i < 8 {
    (*cache)[0] = keys as i64;
    (*cache)[1] = i as i64;
}
```

That `if i < 8` cap is a leftover. The codegen IC fast path computes
`obj + 24 + slot*8` and works for any inline slot, but the cache
update was capped at slot 7. Classes with `field_count > 8` (e.g.
`World` has 16 instance fields, `commandBuffer` is at slot 12) had
every read of `this.commandBuffer` permanently missing the IC: walk
the keys array, do N `js_string_equals`, return value, *don't update
cache*, miss again on the next call.

Fix: change the guard to `i < alloc_limit` (which is `max(field_count,
8)`). Didn't move perf-comprehensive (564 → 563 ms within noise) —
the dominant misses on that workload are non-OBJECT receivers that
fall through earlier — but it's a correctness/perf bug for any class
with > 8 fields and shipping it eliminates a long-tail surprise on
other workloads.

Code: `crates/perry-runtime/src/object.rs::js_object_get_field_ic_miss`.

### `95118ab8` perf(map): GcHeader fast pre-filter for `is_registered_map` (–54 ms)

Profile after the array.push fix:

```
=== TOP LEAF ===
   17.0 %  EntityIdManager.getNextId           (sample-attribution artifact)
   14.0 %  core::hash::BuildHasher::hash_one    ← !!
    8.1 %  core::hash::sip::Hasher::write       ← !!
    4.3 %  quicksort
    1.6 %  set::is_registered_set
    1.3 %  map::is_registered_map
    1.1 %  buffer::is_registered_buffer
```

22 % of CPU was in SipHash. The combined `is_registered_*` lookups
were ~5 % leaf, ~13 % inclusive — most of the SipHash bill. These are
called for every heap-pointer-typed property access in a chain like
`if is_registered_map(addr) { ... } else if is_registered_set(addr) { ... }`
and most calls return false.

Maps are `gc_malloc`'d with `GcHeader.obj_type = GC_TYPE_MAP` (= 8) at
`addr - GC_HEADER_SIZE`. A single i8 load + cmp short-circuits non-Map
ptrs before paying the SipHash:

```rust
pub fn is_registered_map(addr: usize) -> bool {
    if addr < 0x1000 + crate::gc::GC_HEADER_SIZE {
        return false;
    }
    unsafe {
        let header = (addr - crate::gc::GC_HEADER_SIZE)
            as *const crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_MAP {
            return false;
        }
    }
    MAP_REGISTRY.with(|r| r.borrow().contains(&addr))
}
```

The `MAP_REGISTRY` HashSet check still runs on byte-matches to defend
against:

1. **Aliasing.** Sets are raw-`alloc()`'d (no GcHeader), so reading
   `addr - 8` for a Set pointer reads malloc bookkeeping; the byte
   happening to be 8 would be a false positive without the registry
   check.
2. **Stale post-sweep ptrs.** `drop_map_index` removes from
   `MAP_REGISTRY` on sweep; the GcHeader byte may persist past the
   registry removal until the slot is reused.

Buffers and Sets don't get the same treatment in this commit — Sets
aren't gc_malloc'd at all, and Buffers have a slab allocator that
complicates the heap-range check.

Result: perf-comprehensive 564 → 510 ms (–9 %, 1.43× → 1.30× of bun).

Code: `crates/perry-runtime/src/map.rs::is_registered_map`.

### `c716fb46` perf(runtime): MAP/SET/BUFFER registries → PtrHasher (–48 ms)

The HashSet check was *still* the dominant cost on byte-match hits
(after the GcHeader pre-filter, the registry sees true Maps almost
always). SipHash itself was the bottleneck on those.

New module: `crates/perry-runtime/src/fast_hash.rs`. Defines:

```rust
const PTR_MIX: u64 = 0x9E37_79B9_7F4A_7C15;  // 2^64 / φ, rounded odd

#[derive(Default, Clone, Copy)]
pub struct PtrHasher;

impl BuildHasher for PtrHasher {
    type Hasher = PtrHasherImpl;
    fn build_hasher(&self) -> PtrHasherImpl { PtrHasherImpl(0) }
}

pub struct PtrHasherImpl(u64);

impl Hasher for PtrHasherImpl {
    fn finish(&self) -> u64 { self.0 }
    fn write_usize(&mut self, n: usize) {
        self.0 = mix((n as u64).wrapping_mul(PTR_MIX));
    }
    // write_u64, byte-stream write similarly
}

pub type PtrHashSet<T> = HashSet<T, PtrHasher>;
pub type PtrHashMap<K, V> = HashMap<K, V, PtrHasher>;
```

(The `mix()` step is added two commits later; see `39e253cd`.)

Compiles down to a single `mul` per `write_usize`. Pointers from a
system allocator are already well-distributed in their middle bits —
collision-resistant hashing buys nothing, and DoS-resistance doesn't
apply because no external input ever reaches these keys.

Applied to:
- `MAP_REGISTRY` (`crates/perry-runtime/src/map.rs`)
- `SET_REGISTRY` (`crates/perry-runtime/src/set.rs`)
- `BUFFER_REGISTRY` and `UINT8ARRAY_FROM_CTOR` (`crates/perry-runtime/src/buffer.rs`)

Result: perf-comprehensive 510 → 462 ms (–9 %, 1.30× → 1.17× of bun).
Combined `hash_one + sip::Hasher::write` leaf samples dropped from
22 % to 9.2 %.

### `bc646850` perf(gc): MALLOC_STATE.set → PtrHasher (–7 ms)

Same treatment for the GC's malloc-tracked pointer set. `MALLOC_STATE.set`
gets a `HashSet::insert(header_addr)` on every `gc_malloc` (Map / String
/ BigInt / Promise / Error / Closure / etc.) and a `contains()` on
every `gc_realloc`. Already a `HashSet<usize>`; just swap the hasher.

Code: `crates/perry-runtime/src/gc.rs::MallocState`.

### `39e253cd` perf(map): MAP_INDEX → PtrHasher + xorshift avalanche (–6 ms, with a story)

`MAP_INDEX` is a thread-local `HashMap<map_ptr, HashMap<NumericKey, u32>>`
that gives `Map.get`/`Map.has` an O(1) numeric-key path. `World.entityToArchetype.has(entityId)`
runs ~60 k times per `perf-comprehensive` round.

The first attempt at swapping it to PtrHasher caused a **2× regression**
(perf-comprehensive 455 → 830 ms). The cause is the most surprising
thing this iteration found:

- `n.wrapping_mul(MIX)` puts entropy in the **upper** bits of the
  product (Fibonacci hashing's whole point).
- `std::collections::HashMap` reads bucket index from the **lower**
  bits: `hash & (capacity - 1)`.
- `NumericKey` holds **f64 bits**. For whole-number EntityIds, the
  mantissa is zero — `f64(1024.0)` is `0x4090_0000_0000_0000`,
  `f64(1025.0)` is `0x4090_0040_0000_0000`. Many low bits are zero.
- So `f64(1024..15000) * MIX` produces hashes whose low ~50 bits are
  zero. Every key hashes to bucket 0. A 60 k-entry Map.has degrades
  from O(1) to O(n).

Fix: add a one-cycle xorshift avalanche to PtrHasher that mixes the
upper bits down before the bucket-mask sees them:

```rust
#[inline(always)]
fn mix(h: u64) -> u64 {
    h ^ (h >> 32)
}

fn write_u64(&mut self, n: u64) {
    self.0 = mix(n.wrapping_mul(PTR_MIX));
}
```

Cheap on the heap-pointer-keyed registries (which had well-distributed
low bits already and don't *need* the avalanche) but defangs the
integer-f64 collision case so MAP_INDEX can use PtrHasher safely.

Code: `crates/perry-runtime/src/fast_hash.rs::mix` and
`crates/perry-runtime/src/map.rs::MAP_INDEX`.

After landing: perf-comprehensive 455 → 449 ms.

### `582cb5e6` perf(set): SET_INDEX → PtrHasher (–4 ms)

Mirror of `39e253cd` for Set's numeric-key index
(`HashMap<set_ptr, HashMap<JSValueKey, u32>>`). `JSValueKey` hashes
either string content or f64 bits; both work fine under
PtrHasher's avalanche-augmented mixer.

Code: `crates/perry-runtime/src/set.rs::SET_INDEX`.

### `531179ff` perf(runtime): TYPED_ARRAY_REGISTRY + OVERFLOW_FIELDS → PtrHasher (**–33 ms** — biggest single jump)

Two more thread-local pointer-keyed maps. The OVERFLOW_FIELDS one was
the surprise of the run.

`OVERFLOW_FIELDS: HashMap<usize, Vec<u64>>` stores per-class
overflow-slot data for objects that exceed their inline field count
(>8 fields). The non-obvious driver:

> The GC sweep walks every arena object via
> `arena_walk_objects_with_block_index`, calling `clear_overflow_for_ptr(ptr)`
> on each one to drop stale overflow entries.

With ~60 k object literals per perf-comprehensive round (the
`{ type, entityId, componentType, component }` command literals), the
per-call SipHash on `OVERFLOW_FIELDS.remove()` amortized to ~30 ms
across the run. The actual `OVERFLOW_FIELDS` map almost always returns
"not present" — these are inline-field-only literals — but the
SipHash fired on every check.

Result: perf-comprehensive 445 → 412 ms (–7 %, 1.12× → **1.04× of bun**).

Code:
- `crates/perry-runtime/src/object.rs::OVERFLOW_FIELDS`
- `crates/perry-runtime/src/typedarray.rs::TYPED_ARRAY_REGISTRY`

That landed under the 1.10× bar. All 6 ECS workloads green.

## What didn't work, with reasoning

These are recorded so future perf-investigation iterations don't
re-walk these paths without new evidence.

### Direct PropertyGet receiver inlining (Lever A — dead end)

Tried twice in earlier iterations: once via materializing the receiver
into a `Let __recv: Named(C) = chain` and inlining; once via
substituting the receiver expression directly into every `Expr::This`
in the inlined body (no shadow frame slot). Both regressed sync-hotpath
57 → 86 ms (+50 %) and added +25 % to perf-comprehensive.

The runtime `js_native_call_method` IC dispatch ends up cheaper at
scale than the typed-chain `PropertyGet` codegen LLVM emits for the
inlined body, especially for allocation-heavy void methods like
`CommandBuffer.set` (object literal in body). The plumbing
(`class_field_types`, `resolve_receiver_class`) is still in the tree
(commit `43fbdc00`) for selective enabling on alloc-free shapes — don't
re-enable globally.

### Codegen-side fast path for `<arr>.push(v)` on PropertyGet receivers

Looked tractable for `this.commands.push(...)` until I noticed
`crates/perry-hir/src/lower/expr_call.rs::4029-4044` already converts
`expr.push(value)` to `Expr::NativeMethodCall { module: "array" }` via
the AST-level rewrite, which then routes through the existing
`native.rs` handler. Adding a parallel codegen path is unreachable in
the common case — the HIR rewrite catches it first.

### Bumping `GC_THRESHOLD_INITIAL_BYTES` from 64 MB → 128 MB

Tried it: would skip the first GC cycle (which fires mid-bench at
~65 MB total). Saved ~6 ms on perf-comprehensive but at the cost of
RSS regression on `bench_json_roundtrip` (+38 %). Not worth the
trade. Reverted.

### Splitting `gc_check_trigger` into inline fast-path + cold body

The fast-path-only check looks simple, but the function has *two*
trigger conditions (bytes-based and malloc-count-based), and the
malloc-count path has to run even when bytes don't trigger. The
RefCell::borrow on `MALLOC_STATE` in the fast path also raised concerns
about reentrancy with the borrow_mut inside `gc_malloc` itself. Got
tangled and reverted; would need a careful re-think of both trigger
paths before another attempt.

### Set → gc_malloc'd with GC_TYPE_SET

Sets currently use raw `alloc()`, have no GcHeader, and never free
their backing memory. Routing through `gc_malloc` would (a) enable
the same `is_registered_set` GcHeader byte-pre-filter that landed for
Map, and (b) plug a real memory-leak correctness hole. Touches gc.rs
(trace_set + drop_set in sweep), set.rs (alloc), and every Set path
(instanceof checks, etc.). Higher effort than the budget allowed; the
PtrHasher swap covered most of the perf gain anyway.

## Patterns that emerged

1. **Profile before tuning.** The actual hot spots — SipHash on
   pointer-keyed registries; `OVERFLOW_FIELDS` per-object during GC
   sweep — were nowhere near where reading the code would suggest. The
   `EntityIdManager.getNextId` red herring (17-22 % leaf samples on a
   function that's only called from test files) cost a few iterations
   of investigation; verifying with disassembly + source is the antidote.

2. **Pointer-keyed sets/maps don't need SipHash.** The pattern is
   mechanical once you see it: any `HashMap<usize, V>` /
   `HashSet<usize>` keyed by raw heap pointers from a system allocator
   can swap to `PtrHasher` for ~10× cheaper hashing with no loss of
   correctness. The hottest such structures live in:
   `crates/perry-runtime/src/{map,set,buffer,gc,object,typedarray}.rs`.

3. **Multiplicative hash on integer-encoded f64 is a footgun.** Pure
   `n * MIX` puts entropy in the upper bits, but `HashMap` masks the
   low bits for bucket index. Whole-number f64 values (mantissa = 0)
   collapse to bucket 0. Always xor-mix the upper bits down before
   letting `HashMap` do its bucket arithmetic. The `mix(h) = h ^ (h >> 32)`
   step in `PtrHasher` is what makes it safe for non-pointer keys.

4. **The biggest wins were sometimes the most boring.** The single
   largest commit by impact was `531179ff` (–33 ms), which is six
   lines of "swap one HashMap for another." The driver wasn't
   sophisticated codegen — it was that the GC sweep walked every
   arena object once per cycle, paying SipHash on each. Cheap fix,
   big payoff.

## Where to go next if more is needed

If a future workload pushes back over 1.10×, the levers in approximate
order of effort/impact:

1. **More pointer-keyed registries.** A few SipHash-using HashMaps
   remain in perry-runtime that *might* matter on workloads with hot
   patterns we haven't profiled: `STREAM_REGISTRY` (fs.rs),
   `REGEX_POINTERS` (regex.rs), `REMEMBERED_SET` and `CONS_PINNED`
   (gc.rs), `SYMBOL_POINTERS` and `SYMBOL_PROPERTIES` (symbol.rs),
   `CLOSURE_PROPS` (closure.rs). The PtrHasher swap is mechanical.
2. **Convert Set to gc_malloc'd.** As described in "what didn't work."
3. **Reduce IC miss handler work for non-OBJECT receivers.** The
   handler at `crates/perry-runtime/src/object.rs::js_object_get_field_ic_miss`
   could short-circuit common `arr.length` / `str.length` / `map.size`
   paths via gc_type tag inline, before falling through to
   `js_object_get_field_by_name`. ~15-30 ms potential.
4. **Finish the evac path (`PERRY_GEN_GC_EVACUATE=1`).** Off by default,
   and broken — fails after ~1 round with "Component type 1 is not in
   this archetype." The blocker is correctness in `rewrite_forwarded_references` /
   `drain_trace_worklist_inner` (gc.rs). Multi-day work; would let
   minor sweep walk only the unforwarded young set instead of every
   nursery object.

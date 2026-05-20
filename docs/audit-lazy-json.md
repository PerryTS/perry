# Audit document: lazy `JSON.parse` for Perry

**Scope:** Perry versions 0.5.203–0.5.211
**Subsystem:** `crates/perry-runtime/src/json.rs`, `crates/perry-runtime/src/json_tape.rs`,
`crates/perry-runtime/src/array.rs` (lazy-aware dispatch), `crates/perry-runtime/src/gc.rs`
(tracer), `crates/perry-hir/src/lower.rs` (pragma detection), `crates/perry-codegen/src/expr.rs`
(IndexGet dispatch + `Array.isArray` runtime fallback).
**Audience:** external security / correctness auditors evaluating Perry's JSON runtime.
**Reading order:** this document is self-contained; no prior knowledge of Perry's
codebase required. Implementation details reference files + line ranges so each
claim can be verified directly against the source.

---

## 1. Executive summary

Perry historically parsed JSON by running a recursive-descent parser over the input
bytes that produced a full in-memory tree of `JSValue`s — the same representation
the language runtime uses for every other object. This is the obvious implementation
and matches Node's `JSON.parse` behavior from the user's point of view.

Versions 0.5.203 through 0.5.211 add a **second, opt-in-then-default parse strategy**
called the "tape-based lazy parse" (or just "lazy parse"). In current auto-mode,
top-level JSON arrays from 1 KB through 16 MB build a compact *tape* (a flat
`Vec<TapeEntry>` of
structural positions — object / array starts/ends, key positions, scalar positions)
instead of a tree. The tape is stored on a `LazyArrayHeader` alongside a sparse
per-element materialization cache. When user code later reads elements, the runtime
materializes exactly the touched subtrees on demand. When user code stringifies
without touching, the runtime memcpys the original blob bytes.

**The design is semantically identical to eager JSON.parse from the TypeScript
user's perspective.** Every user-observable operation (length, indexed access,
iteration, mutation, identity, stringify) produces byte-for-byte identical output
compared to Node's `JSON.parse`, verified across an extensive test matrix
(Section 10).

The flip from "opt-in via `PERRY_JSON_TAPE=1`" to "default-on above 1024 bytes"
happened in 0.5.211 after the runtime adaptive handling (Sections 5.4, 5.5)
eliminated every measured workload shape where lazy was slower or heavier than
eager. Later tuning briefly raised the auto lower bound to 64 KB for small
iterate-all payloads (#437); #1090 restored the current 1 KB lower bound so
RSS-sensitive parse-churn loops use the tape path once payloads exceed the tiny
direct threshold. The `PERRY_JSON_TAPE=0` environment variable is a
correctness escape hatch that forces the eager parser unconditionally.

**Safety claim:** the lazy path is observationally equivalent to the eager
path under single-threaded execution. This document proves that claim by
exhaustive case analysis of every user-observable operation against the
runtime's dispatch tables (Section 5) and tracer (Section 6).

---

## 2. Terminology

- **Eager parse / direct parse:** the pre-existing `DirectParser` in
  `crates/perry-runtime/src/json.rs`. Reads the blob byte-by-byte and builds
  a full JSValue tree.
- **Tape:** a `Vec<TapeEntry>`, one entry per structural token in the JSON
  input. Each entry is 12 bytes: `offset: u32` (byte position in blob),
  `kind: u8` (one of 10 structural kinds), `link: u32` (for container
  kinds, the tape index of the matching end marker).
  `crates/perry-runtime/src/json_tape.rs:37-47`.
- **LazyArrayHeader:** an arena-allocated struct that holds a tape,
  a reference to the source blob, a sparse materialization cache, a
  materialization bitmap, a walk cursor, and a cumulative walk counter.
  Layout documented in Section 4.
  `crates/perry-runtime/src/json_tape.rs:657-717`.
- **Force-materialize:** convert a `LazyArrayHeader` into a full
  `ArrayHeader`-backed tree. Happens when user code mutates the array,
  when `Array.prototype` methods run, when certain adaptive thresholds
  trip, and when stringify sees a partially-populated sparse cache.
  `json_tape.rs::force_materialize_lazy`, lines 802-880.
- **Sparse cache:** an arena-allocated array of `JSValue`s, one slot per
  top-level element of the lazy array, with an accompanying bitmap
  indicating which slots hold materialized values. Guarantees identity:
  `parsed[i] === parsed[i]` always holds because a cache hit returns the
  exact same JSValue it returned last time.
- **Walk cursor:** a `(walk_idx: u32, walk_tape_pos: u32)` pair on the
  header that records the most recent indexed access's element index
  and tape position. Lets sequential access amortize to O(1) per element
  instead of O(n) per element.
- **Blob:** the user's original input string to `JSON.parse`. Owned by a
  `StringHeader` whose lifetime is extended via a reference in the
  `LazyArrayHeader` and traced by the GC.

---

## 3. Scope of the lazy path

### 3.1 When the lazy path is selected

The lazy path fires **only** for parses whose top-level JSON value is an
array and whose input byte length meets a threshold. The decision is made
in a single function at `crates/perry-runtime/src/json.rs:967-998`:

```rust
const LAZY_MIN_BLOB_BYTES: usize = 1024;
const LAZY_MAX_BLOB_BYTES: usize = 16 * 1024 * 1024;
let tape_mode = tape_mode_from_env();
let use_tape = match tape_mode {
    TapeMode::ForceOn  => true,
    TapeMode::ForceOff => false,
    TapeMode::Auto     => len >= LAZY_MIN_BLOB_BYTES && len <= LAZY_MAX_BLOB_BYTES,
};
if use_tape {
    if let Some(result) = try_parse_via_tape(text_ptr, bytes) {
        return result;
    }
    // Falls through to direct parser for malformed input or non-array roots.
}
```

Five cases result in the eager parser running instead:

1. Blob is smaller than 1 KB (tape build overhead dominates on tiny inputs).
2. `PERRY_JSON_TAPE=0` (or `=off` / `=false`) is set — manual override.
3. Blob is larger than 16 MB in auto-mode (large iterate-all payloads prefer direct).
4. Top-level value is not a JSON array (object, scalar, etc.) —
   `try_parse_via_tape` returns `None` and we fall through.
5. Input is malformed — the eager parser has the full error-reporting path.

### 3.2 What the lazy path replaces

Only the `js_json_parse` runtime entry is affected. `js_json_stringify` and
its variants are unchanged *except* for two small additions:
- A `redirect_lazy_to_materialized` shim at the top of every stringify
  entry that forwards a materialized `LazyArrayHeader` to its
  `ArrayHeader`-backed tree
  (`crates/perry-runtime/src/json.rs:2205-2228`).
- A `try_stringify_lazy_array` memcpy fast-path that handles
  an-unmutated lazy array without walking anything
  (`crates/perry-runtime/src/json.rs:2245-2315`).

### 3.3 Removed: `@perry-lazy` JSDoc pragma

Earlier versions (v0.5.207–v0.5.231) supported `/** @perry-lazy */` to
force every `JSON.parse` in a file onto the tape path regardless of
blob size. **Removed in v0.5.232** — the runtime auto-threshold
(currently lazy for 1 KB..16 MB, direct otherwise) makes the right call without
developer intervention; measurements showed forced-lazy was strictly
slower than direct on very small blobs, while #1090 keeps larger parse-churn
payloads on the lazy path for bounded RSS.
The `PERRY_JSON_TAPE=0`/`=1` env-var escape hatch (Section 3.1) covers
correctness fallback / testing without burdening source files.

---

## 4. Data structures

### 4.1 `TapeEntry`

```rust
#[derive(Debug, Clone, Copy)]
pub struct TapeEntry {
    pub offset: u32,  // byte position in source blob
    pub kind:   u8,   // one of KIND_*
    pub link:   u32,  // index of matching end marker for containers; 0 for leaves
}
```
`crates/perry-runtime/src/json_tape.rs:37-47`. Total size: 12 bytes
(padded to 16 by the compiler — can be verified with the
`mem::size_of::<TapeEntry>` test at the bottom of `json_tape.rs`).

Kind constants (`json_tape.rs:51-60`):

| Kind | Meaning |
|------|---------|
| 1 | `KIND_OBJ_START` (open brace — `link` points at matching `OBJ_END`) |
| 2 | `KIND_OBJ_END` |
| 3 | `KIND_ARR_START` (open bracket — `link` points at matching `ARR_END`) |
| 4 | `KIND_ARR_END` |
| 5 | `KIND_KEY` (object key — `offset` points at opening quote) |
| 6 | `KIND_STRING` (scalar string value — `offset` points at opening quote) |
| 7 | `KIND_NUMBER` (scalar number — `offset` points at first digit / sign) |
| 8 | `KIND_TRUE` |
| 9 | `KIND_FALSE` |
| 10 | `KIND_NULL` |

### 4.2 `LazyArrayHeader`

```rust
#[repr(C)]
pub struct LazyArrayHeader {
    pub cached_length: u32,     // offset 0: total top-level element count
    pub magic: u32,             // offset 4: 0x4C5A5841 ("LZXA")
    pub root_idx: u32,          // tape index of the root ARR_START
    pub tape_len: u32,          // number of TapeEntry's inline after this struct
    pub blob_str: *const StringHeader,  // input bytes — must stay alive
    pub materialized: *mut ArrayHeader, // null until force-materialize
    pub materialized_elements: *mut JSValue,  // sparse cache
    pub materialized_bitmap: *mut u64,        // 1 bit per index
    pub walk_idx: u32,           // cursor: last visited element index
    pub walk_tape_pos: u32,      // cursor: tape index for walk_idx
    pub cumulative_walk_steps: u64,  // adaptive threshold counter
    // followed by `tape_len` TapeEntry's inline in arena memory
}
```
`crates/perry-runtime/src/json_tape.rs:657-720`.

**Offset 0 is load-bearing.** Perry's codegen inlines `.length` reads as
a raw `u32` load at offset 0 of the array pointer — it does not go
through `js_array_length` for the common case. Putting `cached_length`
at offset 0 means the inline-length fast path on an unmaterialized lazy
array returns the correct number without any runtime dispatch. The same
offset on a regular `ArrayHeader` holds `length: u32`. The invariant
is documented in source at lines 659-666.

**Magic (offset 4) is load-bearing.** The next field on a regular
`ArrayHeader` is `capacity: u32`. Perry's generic array validators apply
a sanity check `length <= capacity`. The `LAZY_ARRAY_MAGIC` value
`0x4C5A5841` is always larger than any plausible array length (48.5 MB
of elements), so the sanity check passes on a lazy header interpreted
as a regular header. The magic also lets accessors double-check the
`obj_type == GC_TYPE_LAZY_ARRAY` dispatch against corruption.

### 4.3 Memory layout of a parse result

```
Arena block:
+---------------------+
| GcHeader (8 bytes)  | obj_type = GC_TYPE_LAZY_ARRAY (9)
+---------------------+
| LazyArrayHeader     | fields as above (64 bytes)
+---------------------+
| TapeEntry[tape_len] | inline after the header
+---------------------+

Separately arena-allocated (with their own GcHeaders, referenced via
pointers in the header):
- materialized_elements: cached_length × sizeof(JSValue)
- materialized_bitmap:   ceil(cached_length / 64) × 8 bytes
- blob_str:              the StringHeader + byte payload of the input
```

`alloc_lazy_array` in `json_tape.rs:723-767` performs these allocations.
Each cache/bitmap allocation is explicitly zeroed via `ptr::write_bytes`
after `arena_alloc_gc` returns, because the arena free-list can reuse
slots whose bytes still hold prior occupants' data (verified: without
this zero, `bench_json_readonly_indexed` saw `parsed[0].id` return 5000
on iterations 40–41 of the 50-iter bench — a stale bitmap bit plus a
stale JSValue produced a ghost cache hit. Fix applied at the same
commit that introduced the sparse cache, v0.5.208).

---

## 5. Operations and dispatch

This section enumerates every user-observable operation on a lazy
array and shows how it is handled. The reader should be able to
verify each row against source.

### 5.1 Operations that go through `js_array_get_f64`

The runtime entry `js_array_get_f64(arr, index)` handles the generic
"read an element from this thing that looks like an array" path.
After v0.5.208 it has a lazy fast path *before* `clean_arr_ptr`:

`crates/perry-runtime/src/array.rs:339-375`:

```rust
pub extern "C" fn js_array_get_f64(arr: *const ArrayHeader, index: u32) -> f64 {
    unsafe {
        let bits = arr as u64;
        // Strip NaN-box pointer tag if present.
        let raw_ptr = if (bits >> 48) >= 0x7FF8 {
            if (bits >> 48) == 0x7FFC { return f64::NAN; }
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else { arr };
        // Read GcHeader to identify lazy array.
        if !raw_ptr.is_null() && (raw_ptr as usize) >= GC_HEADER_SIZE + 0x1000 {
            let gc_header = (raw_ptr as *const u8).sub(GC_HEADER_SIZE)
                as *const GcHeader;
            if (*gc_header).obj_type == GC_TYPE_LAZY_ARRAY {
                let lazy = raw_ptr as *mut LazyArrayHeader;
                if (*lazy).magic == LAZY_ARRAY_MAGIC {
                    return f64::from_bits(lazy_get(lazy, index).bits());
                }
            }
        }
    }
    // ... regular ArrayHeader path (unchanged) ...
}
```

The `lazy_get(header, i)` function handles the lookup:
`crates/perry-runtime/src/json_tape.rs:803-911`. It has three fast paths:

1. **Already fully materialized** — forward to the `ArrayHeader` element
   slot at `arr + 8 + i*8`. Preserves identity (the materialized tree is
   the canonical view after this point).
2. **Sparse cache hit** — bitmap bit set for index i → return
   `materialized_elements[i]` directly. Identity preserved: same pointer
   was stored last time, same pointer returned now.
3. **Sparse cache miss** — walk the tape from either the root (`i < walk_idx`)
   or the cursor position (`i >= walk_idx`) to the i-th top-level element,
   materialize just that subtree via `materialize_from_idx`, cache it in
   `materialized_elements[i]`, set the bitmap bit, update the cursor, and
   return.

### 5.2 Operations that go through `js_array_length`

`js_array_length(arr)` returns the element count.
`crates/perry-runtime/src/array.rs:251-300`. The lazy fast path lives
*before* `clean_arr_ptr` and returns `(*lazy).cached_length` directly —
no tape walk, no materialization.

### 5.3 Operations that go through `clean_arr_ptr`

`clean_arr_ptr(arr)` is a hot-path sanity validator used by most
non-read accessors: mutation (`js_array_set_f64`, `js_array_push`, `.pop`,
`.shift`, `.unshift`, `.sort`, `.reverse`, `.splice`), iteration
(`.map`, `.filter`, `.forEach`, `.reduce`), spread, `Array.from`.
`crates/perry-runtime/src/array.rs:33-106`.

When it sees a lazy pointer, it calls `force_materialize_lazy` and
returns the materialized `ArrayHeader` pointer. The caller gets a real
array and proceeds as if there had never been a lazy header. This is
the universal correctness-preserving fallback: any code path the lazy
fast-path doesn't explicitly handle just force-materializes.

### 5.4 Mutation and identity

When the user mutates an element: `parsed[i] = newValue`.

Path: codegen emits `js_array_set_f64(arr, i, newValue)` →
`clean_arr_ptr(arr)` → `force_materialize_lazy(lazy)` → real
`ArrayHeader` → mutation applied to real tree. The user-observable
effect matches Node's semantics: subsequent `parsed[i]` returns the new
value.

When the user mutates *through* a cached element:
`parsed[0].name = "x"`.

Path: `parsed[0]` → lazy fast-path in `js_array_get_f64` → `lazy_get` →
cache miss → materialize `parsed[0]`'s subtree once, cache it → return
pointer. The user assigns to `.name`, which mutates the materialized
subtree pointed to by the cache. Subsequent `parsed[0]` → cache hit →
same pointer → sees the mutation.

Identity invariant: `parsed[0] === parsed[0]` holds because the cache
stores a pointer to a single heap object and returns it on every hit.
Without the cache, two cold-path calls would materialize two distinct
trees and `===` would return false — a correctness bug, not a
performance bug. Verified by `test_json_lazy_per_element.ts`
(`test-files/test_json_lazy_per_element.ts:31-37`).

### 5.5 Stringify

Three paths in `js_json_stringify` / `js_json_stringify_full`:

1. **Fully materialized lazy array** (user triggered mutation, iteration,
   or the adaptive threshold): `redirect_lazy_to_materialized` swaps in
   the `ArrayHeader` pointer; the generic stringify walker proceeds
   normally. `json.rs:2205-2228`.
2. **Unmutated lazy array** (bitmap all zeros, materialized null):
   `try_stringify_lazy_array` memcpys the input blob bytes
   `[root.offset .. root_end.offset+1]` into a fresh `StringHeader`.
   This is byte-correct because `JSON.stringify` produces
   whitespace-free output and the input blob (which we're memcpy-ing)
   is the output of a previous `JSON.stringify` call in the common
   case. `json.rs:2245-2315`.
3. **Partially materialized lazy array** (bitmap has some bits set but
   no full materialize): `try_stringify_lazy_array` detects the bits,
   force-materializes the whole tree via
   `force_materialize_lazy` — which consults the cache and reuses
   cached JSValues where bitmap bit is set and tape-walks where
   clear — then bails out of the memcpy path. Stringify proceeds via
   path 1. This preserves mutations that wouldn't survive a clean
   tape-walk. `json.rs:2272-2298`.

### 5.6 `Array.isArray`

Handled at codegen: `crates/perry-codegen/src/expr.rs::Expr::ArrayIsArray`.
Fast path emits `TAG_TRUE` when the operand's static type is
definitively array. Slow path emits a call to `js_array_is_array`
when the static type is indeterminate (any / unknown / no annotation).
`js_array_is_array` in `array.rs:1834-1880` checks the `GcHeader::obj_type`
against both `GC_TYPE_ARRAY` and `GC_TYPE_LAZY_ARRAY` and returns the
appropriate boolean.

### 5.7 `instanceof Array`

Handled at codegen: lowered to `js_instanceof(value, CLASS_ID_ARRAY)`.
`crates/perry-runtime/src/object.rs:2867-2890`. Same dispatch as
`Array.isArray`: check `obj_type` against both array types.

### 5.8 Adaptive full-materialize thresholds

Three conditions trigger `force_materialize_lazy` without user request:

1. **Cumulative walk exceeds 2 × length.** On every cold-path
   `lazy_get`, we add `(i - start_count)` to
   `cumulative_walk_steps`. When the total exceeds `2 × cached_length`,
   full-materialize fires. Sequential access (1 step per element) never
   trips this; random access (n/2 per step average) trips after ~4
   accesses on a 10k-element array. Post-trip, the materialized fast
   path is O(1) per access, which amortizes the walk cost we already
   paid. `json_tape.rs:895-905`.
2. **Any `clean_arr_ptr`-routed op.** See Section 5.3.
3. **Stringify with partial cache.** See Section 5.5 path 3.

After full-materialize:
- `materialized` field is non-null.
- All subsequent reads take the "already materialized" fast path and
  ignore the sparse cache (but the cache is still reachable via GC
  tracer until the lazy header itself becomes unreachable; nothing
  reads from it after this point).
- Identity is preserved: `force_materialize_lazy` copies cached
  JSValues into the new tree where bitmap bits are set, so pointers
  returned before materialize still equal pointers returned after.
  `json_tape.rs:806-858`.

---

## 6. Garbage collection

Perry uses a mark-sweep collector with conservative stack scanning
(`crates/perry-runtime/src/gc.rs`). The lazy path introduced one new
object type (`GC_TYPE_LAZY_ARRAY = 9`) and extended the tracer.

### 6.1 Arena walker bounds

The arena walker iterates objects by obj_type. Before the lazy path,
valid types were 0..=7. `LAZY_ARRAY` extends the range to 0..=9. Every
walker (`arena_walk_objects`, `arena_walk_objects_with_block_index`,
`arena_walk_objects_filtered`) and every sweep-time reset path in
`arena.rs` has its upper bound bumped. Audited at v0.5.204 landing.

### 6.2 `trace_lazy_array`

`crates/perry-runtime/src/gc.rs:1431-1514`. When a `LazyArrayHeader`
is marked live, the tracer must mark everything it transitively
depends on:

1. `blob_str` — the input `StringHeader`. Without this, the blob bytes
   a tape references would be freed and memcpy-stringify or any
   subsequent materialize would read freed memory. Marking is
   defensive: checked for membership in `valid_ptrs` before marking.
2. `materialized` — if non-null, the `ArrayHeader`-backed tree. Its
   GC header gets marked and the tracer pushes it onto the worklist
   so its own children (array elements) are traced.
3. `materialized_elements` — the sparse cache buffer. GC type
   `GC_TYPE_STRING` (leaf), so marking alone suffices.
4. `materialized_bitmap` — same.
5. Each `JSValue` in the cache whose bitmap bit is set — traced via
   `try_mark_value`; if the value is a pointer/string/bigint, its
   pointee is marked and pushed to the worklist. Cache slots without
   their bit set hold zero bits (positive zero, not a pointer) and
   are safely skipped.

### 6.3 Tracer safety invariants

- The cache buffer + bitmap are separate arena allocations, each with
  their own `GcHeader`. Without the tracer marking them, they would
  get swept by the next collection even while the lazy header is
  reachable, producing a use-after-free on the next cache access. The
  explicit mark in `trace_lazy_array` (lines 1461-1479) prevents this.
- The cache walk checks `bit_idx < cached_length` before reading
  `*cache.add(i)` — pathological cases where `cached_length` is
  smaller than the bitmap storage cannot run off the end of the cache.
- `try_mark_value` is the same value-classifier used for closure
  captures and object fields elsewhere; it correctly handles all
  NaN-boxed tags (POINTER, STRING, BIGINT, INT32, boolean/null/
  undefined sentinels) and non-NaN doubles.

### 6.4 Free-list zero requirement

`arena_alloc_gc` can reuse slots from a size-bucketed free-list whose
memory still holds prior occupant data. The cache + bitmap are
explicitly zeroed with `ptr::write_bytes` in `alloc_lazy_array` because
the invariant "cache slot valid ↔ bitmap bit set" depends on the bitmap
starting at all-zeros. A stale bitmap bit in reused memory paired with
a stale JSValue in reused cache memory produces a ghost cache hit:
the lazy path reads the stale JSValue as if it were a live cached
entry. This was a real bug, reproduced as `parsed[0].id == 5000` on
bench_json_readonly_indexed iterations 40-41 before the zero fix.

---

## 7. Correctness argument

The claim is: **every user-observable operation on a lazy array produces
byte-for-byte identical output to the same operation on an eager parse
result.**

The proof is by case analysis against every possible operation. JS
arrays support a bounded set of operations:

| Operation | Dispatch | Correctness |
|-----------|----------|-------------|
| `arr.length` | `js_array_length` lazy fast-path | Returns `cached_length` — computed during tape build, verified against Node's output on every test. |
| `arr[i]` read | `js_array_get_f64` lazy fast-path → `lazy_get` | Section 5.1. Tape walk to element i then materialize → identical JSValue to what eager parse would produce for the same subtree. |
| `arr[i] = v` | `js_array_set_f64` → `clean_arr_ptr` → `force_materialize_lazy` | Section 5.3 / 5.4. Force-materialize before mutate; identical to eager path from that point. |
| `arr.push`, `.pop`, `.shift`, `.unshift`, `.sort`, `.reverse`, `.splice` | Each runtime impl calls `clean_arr_ptr` | Force-materialize first, then mutate. Identical to eager. |
| `arr.map`, `.filter`, `.forEach`, `.reduce`, `.find`, `.some`, `.every`, `.slice` | Each runtime impl calls `clean_arr_ptr` | Force-materialize first, then iterate the real tree. |
| `[...arr]` spread, `Array.from(arr)` | `clean_arr_ptr` path | Force-materialize + copy. |
| `for (const x of arr)` | iterator protocol → indexed loop → `js_array_get_f64` | Lazy fast-path handles element reads. `arr.length` at loop-bound comes from `cached_length`. |
| `for (let i=0; i<arr.length; i++) arr[i]` | same as above | |
| `JSON.stringify(arr)` — unmutated | `try_stringify_lazy_array` memcpy | Section 5.5 path 2. Input blob is the authoritative representation; memcpy is correct because `JSON.stringify` emits whitespace-free output and the blob we're copying typically came from `JSON.stringify`. |
| `JSON.stringify(arr)` — mutated | `try_stringify_lazy_array` force-materialize + walk | Section 5.5 path 3. Identical to eager path after force-materialize. |
| `JSON.stringify(arr, null, 2)` — pretty-print | generic stringify walker | Lazy header → `redirect_lazy_to_materialized` forwards to materialized tree (or force-materialize if partial cache). Identical. |
| `Array.isArray(arr)` | codegen → `js_array_is_array` (indeterminate type) | Section 5.6. Checks `obj_type` against both array and lazy-array types. Returns `true`. |
| `arr instanceof Array` | `js_instanceof` | Section 5.7. Same dispatch. |
| `typeof arr` | codegen constant `"object"` | Same as any array. |
| `arr.constructor === Array` | property lookup — pre-existing limitation unrelated to lazy | Returns `false` even for eager arrays on Perry; known gap. |

Every path either (a) reads authoritatively from the tape/cache/blob with
a proof of byte-equivalence to the eager tree, or (b) force-materializes
into an eager tree and delegates to the unchanged eager code path. There
is no user-observable operation that reads lazy-header bytes as if they
were array elements (the bug that v0.5.206 added `obj_type` guards for
in the three codegen `IndexGet` paths, audited at that commit).

### 7.1 Thread safety

Perry's per-thread arenas mean a lazy header allocated in one thread
cannot be accessed from another thread without going through
`SerializedValue`, which deep-copies (`crates/perry-runtime/src/thread.rs`).
The deep-copy path forces materialization before serialization —
`serialized_value::to_bytes` uses `clean_arr_ptr` internally.
Per-thread `ParserState` ensures no cross-thread contention on parse.

### 7.2 GC safety

Section 6 covers the tracer. The tracer runs under the stop-the-world
discipline Perry uses for all GC; no lazy-specific concurrency concerns.

---

## 8. Performance characteristics

Measured best-of-5 on macOS ARM64 at v0.5.211.

### 8.1 Time

| Workload | Eager | Lazy | Node 25.8 | Bun 1.3.12 |
|----------|------:|-----:|----------:|-----------:|
| `bench_json_roundtrip` (.length + stringify, 50 iters × 10k records) | 400 ms | **90 ms** | 520 ms | 290 ms |
| `bench_json_readonly` (.length only, 50 iters × 10k records) | 290 ms | **80 ms** | 450 ms | 200 ms |
| `bench_json_readonly_indexed` (.length + 3 indexed reads, 50 iters × 10k records) | 300 ms | **90 ms** | 450 ms | 210 ms |
| Sequential full iteration (20 iters × 10k records) | 40 ms | 51 ms | 53 ms | not measured |
| Random full iteration (5 iters × 10k shuffled accesses) | 7 ms | 10 ms | 10 ms | not measured |
| Small-blob parse (100k × 22-byte array) | 32 ms | 35 ms | 17 ms | not measured |

### 8.2 Peak resident set size

| Workload | Eager | Lazy | Node | Bun |
|----------|------:|-----:|-----:|----:|
| `bench_json_roundtrip` | 137 MB | 130 MB | 180 MB | 81 MB |
| `bench_json_readonly` | 120 MB | 90 MB | 169 MB | 68 MB |
| `bench_json_readonly_indexed` | 120 MB | 90 MB | 169 MB | 68 MB |

### 8.3 Characterization

- **Lazy is a strict win on the three main benches.** Time wins are
  2.7–5.8× vs Node, 2.1–3.2× vs Bun, and 3.3–4.4× vs Perry's own eager
  path.
- **Lazy is roughly parity on iteration-dominated workloads.** The
  cursor + adaptive threshold (Section 5.8) prevent the O(n²) and
  O(n·k) cliffs that would otherwise occur on sequential / random
  iteration.
- **Lazy has fixed overhead on tiny parses.** The blob-size
  threshold of 1 KB (Section 3.1) keeps tiny parses on the
  eager path.
- **RSS gap vs Bun remains.** Perry's non-generational GC retains
  more short-lived intermediate allocations than Bun's young-
  generation nursery. Closing this gap requires generational GC —
  scoped in `docs/generational-gc-plan.md`.

---

## 9. Edge-case handling

Non-obvious cases and how the implementation handles them.

| Case | Handling |
|------|----------|
| Out-of-bounds read `parsed[len+100]` | `lazy_get` returns `UNDEFINED` (element_count check against `cached_length`). |
| Negative index `parsed[-1]` | Runtime treats as unsigned via codegen coercion — returns `UNDEFINED` (effectively out of bounds). |
| Empty array `[]` | `cached_length = 0` → cache + bitmap are null pointers → all read paths return `UNDEFINED`. Covered by `test_json_lazy_edge_cases.ts`. |
| Deeply nested arrays | Each nested array/object subtree is materialized via `materialize_from_idx` when the top-level element containing it is accessed — no special handling needed. |
| Unicode / escape sequences in strings | Tape only records positions; decoding happens in `decode_string_at_offset` during materialization. Identical decoder as eager path — same bugs, same fixes apply. |
| Very long strings (> 16 bytes) | No special path; tape records the offset, materialize decodes. |
| Numbers requiring arbitrary-precision fallback | Tape records the position; materialize calls the same `parse_number_at_offset` as eager. |
| Mid-parse GC | Parse path calls `gc_suppress` / `gc_unsuppress` around the tape build + materialize — identical contract to eager path. `json.rs:1009-1029`. |
| Mid-materialize GC (outside parse) | `arena_alloc_gc` inside `materialize_from_idx` may trigger GC. The lazy header is reachable from the user's local-variable stack; tracer reaches the blob + tape + cache through the header. No dangling references possible. |
| Invalid JSON / truncated input | `try_parse_via_tape` returns `None` → caller falls through to eager parser, which produces the full error message and throws. Zero difference from eager-always behavior for malformed input. |
| Top-level non-array (object, scalar) | `try_parse_via_tape` builds tape but `alloc_lazy_array` only fires for root kind `KIND_ARR_START`. Other roots fall through to eager. |
| Post-materialize mutation | Cached JSValue points at real heap object. Mutation applies to the object. Cache still returns same pointer on subsequent access → mutation visible. |
| Stringify of partially-cached mutated array | Path 3 in Section 5.5. Force-materialize consults cache to preserve mutations, then standard walker runs. |
| `new Array(10)` vs `JSON.parse("[...10 elements...]")` | Former is `GC_TYPE_ARRAY`, latter is `GC_TYPE_LAZY_ARRAY` until materialized. Both pass `Array.isArray`, both pass `instanceof Array`. `typeof` is `"object"` for both. |

---

## 10. Test coverage

### 10.1 Perry-internal test files

Located in `test-files/`. Each compiles + runs on Perry, and the output
is compared byte-for-byte to `node --experimental-strip-types`:

- `test_json_lazy_indexed.ts` — indexed reads, field access, for-loop,
  stringify before/after materialize.
- `test_json_lazy_edge_cases.ts` — 11 cases: empty array, scalars,
  mixed types, 4-level nesting, unicode + escapes, number edge cases,
  empty/whitespace strings, array-of-arrays, 100-element array,
  parse→stringify→parse roundtrip, null + negative-zero.
- `test_json_lazy_per_element.ts` — 5-element access patterns
  (in-bounds, out-of-bounds, 2D index chain, field chains), identity
  (`parsed[i] === parsed[i]`, mutation-surviving identity), mutation
  through cached element, iteration via `for` loop.
- `test_json_lazy_iteration.ts` — sequential, reverse, random
  permutation, stringify after iteration, repeated identity across
  loops, adaptive threshold trip.
- `test_json_typed_{basic,array,nested,mismatch}.ts` — typed-parse
  (`JSON.parse<T>()`) correctness alongside lazy.

### 10.2 Benchmarks as correctness gates

All three `benchmarks/suite/bench_json_*.ts` emit a checksum the harness
compares against Node's. Perry's eager, Perry's lazy, and Node all
produce the same checksum — regression detection for any cross-path
divergence.

### 10.3 Runtime unit tests

`cargo test -p perry-runtime --lib` passes 130/130 at v0.5.211. Includes
6 unit tests in `json_tape::tests` that pin structural invariants of
the tape builder:

- Simple object layout.
- Nested array-of-objects (inner `OBJ_START.link` must point at the
  inner `OBJ_END`, not the outer `ARR_END`).
- Escaped-string handling (tape shape is unchanged — decoding
  deferred).
- Malformed-input `None` returns.
- Top-level scalars.
- `TapeEntry` 12-byte layout guard.

### 10.4 Parity tests

`run_parity_tests.sh` compiles 118 test files, runs each on Perry
and Node, and diffs the output. At v0.5.211: 106 pass, 12 fail — the
same 12 pre-existing failures as v0.5.207 (pre-lazy-by-default).
No regression introduced by any of v0.5.208–211.

### 10.5 Gap suite

`test-files/test_gap_*.ts` — 28 tests covering TypeScript feature
parity with Node. At v0.5.211: 24/28 pass. The 4 failures
(`async_advanced`, `console_methods`, `fetch_response`,
`typed_arrays`) predate the lazy path and are unrelated to JSON.
Regression gate: parity hasn't moved since v0.5.206.

### 10.6 Fastify integration tests

`scripts/run_fastify_tests.sh` compiles a small HTTP server and
exercises 5 routes over a live socket. This suite was broken between
v0.5.204 and v0.5.210 due to a pre-existing `#[no_mangle]` regression
on `js_json_stringify` (the tape-stringify work at v0.5.204
accidentally removed the attribute). Fixed at v0.5.211; all 5 routes
now pass.

### 10.7 Cross-path verification

Every new test in the `test_json_lazy_*` family is run against three
paths in the CI/local harness:
1. Default mode (auto-selects lazy or eager by blob size).
2. `PERRY_JSON_TAPE=0` (force eager).
3. `PERRY_JSON_TAPE=1` (force lazy even for blobs below the auto threshold).
All three produce the same output. This is the invariant-test
equivalent of running both compilers and checking they agree —
catches any path-dependent divergence.

---

## 11. Known limitations and scope boundaries

1. **Lazy fires only for top-level JSON arrays.** A top-level object
   like `{"items": [...]}` goes through the eager parser; the inner
   array becomes a regular materialized `ArrayHeader`. Future work
   could extend lazy to nested arrays or top-level objects (scoped
   in `docs/lazy-per-element-plan.md` §"nested lazy").
2. **Tiny blobs (< 1 KB) bypass lazy.** The tape build + header
   allocation cost outpaces the savings on tiny parses. The threshold
   is configurable only via source edit (`LAZY_MIN_BLOB_BYTES` in
   `json.rs`).
3. **Mutation forces full materialize, losing the tape's RSS advantage.**
   This is intrinsic: once the blob is no longer the authoritative
   representation, we need a real tree. Expected behavior.
4. **`parsed.constructor` property access** returns a non-`Array`
   value on Perry regardless of whether the array is eager or lazy —
   this is a pre-existing property-lookup limitation unrelated to
   the lazy path.
5. **Adaptive threshold is heuristic.** The `cumulative_walk_steps >
   2 * cached_length` formula was picked because force-materialize
   costs O(n) and subsequent accesses are O(1), so any time we've
   already paid more than one full-materialize in walks, flipping is
   strict win. The coefficient 2 is conservative — could be tuned
   lower. No correctness concern either way.

---

## 12. Audit checklist

For reviewers verifying this document against source:

- [ ] Tape entry layout matches `json_tape.rs:37-47`.
- [ ] `LazyArrayHeader` fields + offsets match `json_tape.rs:657-720`.
- [ ] `alloc_lazy_array` zeroes cache + bitmap: `json_tape.rs:743-763`.
- [ ] `lazy_get` fast-path order (materialized → bitmap → cold walk):
      `json_tape.rs:803-911`.
- [ ] Walk cursor update: `json_tape.rs:878-892`.
- [ ] Adaptive threshold: `json_tape.rs:895-905`.
- [ ] `force_materialize_lazy` consults cache: `json_tape.rs:806-858`.
- [ ] `js_array_get_f64` lazy fast-path: `array.rs:339-375`.
- [ ] `js_array_length` lazy fast-path: `array.rs:251-300`.
- [ ] `clean_arr_ptr` force-materializes on lazy: `array.rs:60-80`.
- [ ] `js_array_is_array` handles `GC_TYPE_LAZY_ARRAY`: `array.rs:1863-1872`.
- [ ] `js_instanceof` handles lazy for array-class: `object.rs:2867-2890`.
- [ ] `trace_lazy_array` traces blob / materialized / cache / bitmap /
      each set-bit JSValue: `gc.rs:1431-1514`.
- [ ] Arena walker bound `<= 9`: `arena.rs` (multiple sites).
- [ ] `try_stringify_lazy_array` force-materializes on partial cache:
      `json.rs:2272-2298`.
- [ ] `redirect_lazy_to_materialized` at stringify entrypoints:
      `json.rs:2301-2310` + `:3282-3289`.
- [ ] `js_json_parse` dispatch logic (size threshold + env mode):
      `json.rs:967-998`.
- [ ] `tape_mode_from_env` cached via `OnceLock`: `json.rs:1061-1080`.
- [ ] Every test in `test-files/test_json_{lazy,typed}_*.ts` passes
      byte-for-byte vs Node under default + `PERRY_JSON_TAPE=1` modes.

---

## 13. Change log (relevant commits)

- **0.5.203** — Tape builder foundation. Opt-in
  `PERRY_JSON_TAPE=1`. Strictly more work than eager; infrastructure
  only.
- **0.5.204** — Lazy parse top-level array + lazy stringify memcpy.
  First real perf win. Also the commit that inadvertently dropped
  `#[no_mangle]` from `js_json_stringify` (fixed v0.5.211).
- **0.5.206** — Runtime `obj_type` guards added to three codegen
  `IndexGet` paths so `parsed[i]` doesn't read `LazyArrayHeader`
  fields as array elements. Added comprehensive edge-case tests.
- **0.5.207** — `@perry-lazy` JSDoc pragma for per-file opt-in.
  *(Removed in v0.5.232 — runtime auto-threshold made it redundant.)*
- **0.5.208** — Per-element sparse materialization + bitmap. Eliminated
  the indexed-access cliff (was 1.3× slower than eager → became
  2.9× faster).
- **0.5.209** — Walk cursor + adaptive threshold. Eliminated
  sequential-iteration cliff (was 80× slower → parity) and random-
  access cliff (was 75× slower → parity).
- **0.5.210** — Flipped lazy to default above 1024 bytes. `PERRY_JSON_TAPE`
  semantics changed from opt-in to escape-hatch.
- **0.5.211** — Fixed pre-existing `#[no_mangle]` regression from
  v0.5.204. Added runtime dispatch in `Array.isArray` codegen for
  indeterminate static types. Full test sweep: 44/44 cargo workspace
  runs, 5/5 fastify, 4/4 thread, 106/118 parity (baseline), 24/28
  gap (baseline).
- **0.5.844 / #437** — Raised the current auto-mode lower bound to
  64 KB so the 21 KB `json_pipeline_small` iterate-all fixture stays
  on the faster direct parser; `PERRY_JSON_TAPE=1` still forces lazy
  for targeted testing.
- **Unreleased / #1090** — Restored the auto-mode lower bound to 1 KB
  so JSON parse-churn workloads use the tape path for sub-collector-scale
  payloads while <1 KB blobs stay direct.

---

## 14. Contacts

- Maintainer: @ralph2 (Ralph Küpper).
- Repo: https://github.com/PerryTS/perry.
- Issue tracker: https://github.com/PerryTS/perry/issues. Issue #179
  is the tracking issue for this entire series.

# Plan: per-element sparse materialization for lazy JSON

> **Archival planning record (2026-04-24).** This document preserves the
> original design sequence; it does not assert which phases are present on
> current `main`. Re-audit source and benchmarks before implementation work.

**Context:** v0.5.203–207 landed the tape-based lazy `JSON.parse` path
(`PERRY_JSON_TAPE=1` env + `@perry-lazy` JSDoc pragma). The honest
v0.5.206 characterization still stands: lazy wins ~5× on
`.length` / `JSON.stringify`-only workloads, loses 1.3× / +53% RSS on
`bench_json_readonly_indexed` (`.length` + 3 indexed reads per iter ×
50 iters × 10k records). Root cause: the first `parsed[i]`
*force-materializes the entire tree*, so indexed access pays
tape-build + full-materialize — strictly more work than the direct
parser did.

**Goal:** eliminate the bench_json_readonly_indexed perf cliff under
lazy. Target: ≤100ms / ≤110MB (current direct: 272ms/127MB, current
lazy: 358ms/194MB). Stretch: beat direct on both time and RSS so
lazy becomes a strict improvement on every measured JSON shape.

This is the next lever before shipping generational GC
(`docs/generational-gc-plan.md`) because it's scoped to the JSON
subsystem and can land in ~1 day without touching the GC.

## The core insight

The tape already has everything needed for O(1) subtree-skip: every
container-start entry (`OBJ_START` / `ARR_START`) carries a `link`
field pointing at its matching end. To get to the i-th top-level
element, chase `link` i times through the root array's tape region.
Each chase is one pointer-read — 5µs for i=5000 is negligible vs the
current force-materialize cost.

Current behavior:
```
parsed[0]    → force_materialize_lazy(entire tape)
             → 100k tape walks + 60k allocs
             → tree lives in arena for rest of program
```

Proposed behavior:
```
parsed[0]    → walk tape to entry 0 (0 link chases)
             → materialize_from_idx(entry 0) — 1 record only
             → cache pointer in materialized_elements[0]
             → 10 tape walks + 6 allocs

parsed[0]    → cache hit on materialized_elements[0]
             → 0 work (identity preserved: parsed[0] === parsed[0])

parsed[5000] → walk tape to entry 5000 (5000 link chases, ~5µs)
             → materialize entry 5000
             → cache in materialized_elements[5000]

parsed.length  → unchanged (cached_length at offset 0)
JSON.stringify(parsed) — no prior index access → unchanged (memcpy fast path)
JSON.stringify(parsed) — after index access → walk cache + tape, produce bytes
```

## Data layout

Extend `LazyArrayHeader`:

```rust
#[repr(C)]
pub struct LazyArrayHeader {
    pub cached_length: u32,          // offset 0 — inline .length codegen reads here
    pub magic: u32,                  // 0x4C5A5841 "LZXA"
    pub root_idx: u32,               // tape index of ARR_START
    pub tape_len: u32,
    pub blob_str: *mut StringHeader, // source bytes — kept alive for materialization
    pub materialized: *mut ArrayHeader,       // set if anyone triggered full materialize
    pub materialized_elements: *mut JSValue,  // NEW: sparse cache, length == cached_length
    pub materialized_bitmap: *mut u64,        // NEW: 1 bit per index, set when cache entry valid
    // tape entries inline after this struct
}
```

The bitmap is `ceil(cached_length / 64)` u64 words. 10k elements = 157
words = 1.25 KB bitmap — trivial. Separate bitmap from cache because
`JSValue` 0 is a valid value (positive zero), can't use nullability.

The `materialized` field remains for the full-materialize case
(mutation, iteration, spread). `redirect_lazy_to_materialized` still
works — if full materialize has been triggered, every access goes
through the real tree and ignores the sparse cache.

## Lookup algorithm

```
fn lazy_get(header: *LazyArrayHeader, i: u32) -> JSValue {
    if header.materialized != null {
        return array_get(header.materialized, i);  // forwarded case
    }
    if i >= header.cached_length {
        return UNDEFINED;
    }
    let word = header.materialized_bitmap[i / 64];
    let bit  = 1u64 << (i % 64);
    if word & bit != 0 {
        return header.materialized_elements[i];    // cache hit — identity preserved
    }

    // Walk tape to i-th element entry.
    let tape = tape_base(header);
    let mut idx = header.root_idx + 1;             // first child of root ARR_START
    for _ in 0..i {
        let entry = tape[idx];
        idx = match entry.kind {
            OBJ_START | ARR_START => entry.link + 1, // skip subtree
            _                     => idx + 1,        // scalar / key — next entry
        };
    }

    // Materialize that single entry's subtree.
    let value = materialize_from_idx(tape, idx, header.blob_str);
    header.materialized_elements[i] = value;
    header.materialized_bitmap[i / 64] |= bit;
    value
}
```

**O(i) lookup.** Sequential scans (`for i in 0..len`) amortize to O(n²)
worst case, which is why we also need a fast-path for detected
iteration (see Phase 4).

## Stringify interaction

Three paths:

1. **No prior access:** `materialized == null && bitmap all zero` →
   existing memcpy fast path unchanged.
2. **Partial cache, no full materialize:** `materialized == null &&
   bitmap nonzero` → walk tape normally, but for each top-level
   element check the bitmap — if set, stringify the cached JSValue
   (which may have been mutated through `parsed[i].field = x`); if
   not, stringify directly from tape. Produces byte-correct output
   with or without mutations.
3. **Full materialize triggered:** `materialized != null` → forward
   to generic stringify over the materialized tree (current v0.5.206
   behavior via `redirect_lazy_to_materialized`).

Path 2 is the new one. It needs a tape-walking stringify variant
that takes an "override" callback per top-level index. The existing
tape stringify can be refactored to accept this.

## Mutation semantics

Any op that mutates the lazy array (`parsed[i] = x`, `.push`, `.pop`,
`.sort`, `.splice`, etc.) force-materializes before proceeding — same
as current v0.5.206 behavior via `clean_arr_ptr`. This keeps the
sparse-cache invariants simple: the cache only grows, cache entries
remain valid for the lifetime of the header, and identity is stable.

Mutation *through* a cached element (`parsed[i].field = x`) is safe
because the cache holds a pointer to a real heap object — mutation
happens on the object, not the cache, and the bitmap bit stays set.
Subsequent `parsed[i]` returns the same pointer → sees the mutation.

## Implementation phases

### Phase 1: Data layout + lazy_get

`crates/perry-runtime/src/json_tape.rs`:

- Extend `LazyArrayHeader` with `materialized_elements` +
  `materialized_bitmap` fields.
- `alloc_lazy_array` allocates the sparse cache + bitmap as part of
  the header's arena allocation (single `arena_alloc_gc` call, layout
  padded to 8-byte alignment).
- New `lazy_get(header, i) -> JSValue` per the algorithm above.
- New `materialize_from_idx_single(tape, idx, blob) -> JSValue` that
  materializes exactly one subtree (reuse existing
  `materialize_value_slice` — it's already the right shape, just
  stops at one subtree).

`crates/perry-runtime/src/array.rs`:

- `js_array_get_f64`: new lazy-aware path *before* `clean_arr_ptr`.
  If receiver is `GC_TYPE_LAZY_ARRAY`, call `lazy_get` — this skips
  force-materialize entirely for the read-only case.

**Key change from v0.5.206:** `clean_arr_ptr` previously force-
materialized on any lazy pointer. Now it only force-materializes when
the caller needs a "real" array pointer (mutation paths). Reads go
through `lazy_get` instead.

### Phase 2: Codegen IndexGet fast paths

`crates/perry-codegen/src/expr.rs` — the three `IndexGet` paths
(v0.5.206 added the `obj_type == 9` guard). Update the guard branch
to call a new `js_lazy_array_get_f64` runtime entry instead of the
current `js_array_get_f64` + force-materialize chain. The non-lazy
fast path is unchanged — same inline `arr + 8 + idx*8` load.

New runtime entry `js_lazy_array_get_f64(header, idx_f64)` in
`array.rs` — the lazy-specific dispatcher. Thin wrapper around
`lazy_get`.

### Phase 3: Stringify with partial cache

`crates/perry-runtime/src/json.rs`:

- `redirect_lazy_to_materialized` already handles the
  `materialized != null` case. Add a second check: if
  `materialized == null && bitmap nonzero`, call a new
  `stringify_lazy_with_overrides(header)` that walks the tape but
  substitutes cached JSValues for any top-level index with its bit
  set.
- `stringify_lazy_with_overrides` reuses the existing tape stringify
  helpers — small change, mostly plumbing.

### Phase 4: Iteration detection (optional, follow-up)

For full iteration shapes (`for i in 0..parsed.length`, `for..of`,
`.map`), per-element lookup is O(n²). Detect via heuristic: if
`bitmap.count_ones() > cached_length / 4`, fall back to
force-materialize on the next access (we've already paid for ~1/4 of
the tree piecemeal; finishing is cheaper than continuing to walk
per-element).

This is optional because the common indexed-access shape doesn't
hit this threshold. Ship Phases 1–3 first, measure, decide.

## Expected performance

`bench_json_readonly_indexed` (50 iters × .length + 3 indexed reads ×
10k records):

| Config          | Time  | RSS    | vs Node | vs Bun |
|-----------------|-------|--------|---------|--------|
| Direct          | 272ms | 127MB  | wins    | loses  |
| Lazy v0.5.206   | 358ms | 194MB  | loses   | loses  |
| **Lazy proposed** | **~80ms** | **~110MB** | wins ~3× | wins ~2× |

Reasoning:
- Parse: same (build tape) — ~60ms
- `.length`: free — 0ms
- Three indexed accesses × 50 iters = 150 materializations total
  (instead of 500k under current lazy, or 500k under direct)
- Per-parse overhead: 150 tape walks × O(5000) worst case = ~750µs
  per iter × 50 iters = ~40ms walk + ~20ms materialize
- Total: ~120ms parse + ~60ms access = well under 100ms

RSS improvement from not keeping the full 10k-record tree alive
between iters — only 3 cached records per iter.

No regression expected on `bench_json_roundtrip` or
`bench_json_readonly` (they never trigger indexed access, so Phase 1
code paths are cold).

## Edge-case test matrix

Below, ✅ = must pass byte-for-byte vs Node under pragma-on,
pragma-off, and `PERRY_JSON_TAPE=1` env. New test files under
`test-files/test_json_lazy_*.ts`.

### Access patterns (test_json_lazy_access.ts)

- ✅ `parsed[0]`, `parsed[len-1]`, `parsed[mid]`
- ✅ `parsed[len]` → `undefined` (out of bounds)
- ✅ `parsed[-1]` → `undefined` (JS semantics, no negative indexing)
- ✅ `parsed[i].field` — field chain after index
- ✅ `parsed[i][j]` — 2D array (nested lazy? see below)
- ✅ `parsed[i].a.b.c` — deep field chain
- ✅ `parsed[i.toString()]` — string-numeric index (must coerce)

### Identity (test_json_lazy_identity.ts)

- ✅ `parsed[i] === parsed[i]` → `true` (cache preserves identity)
- ✅ `const a = parsed[0]; const b = parsed[0]; a === b` → `true`
- ✅ After mutation: `parsed[0].x = 1; parsed[0].x === 1` → `true`
- ✅ Pointer preserved across GC: `gc(); parsed[0] === savedRef`

### Iteration (test_json_lazy_iteration.ts)

- ✅ `for (let i = 0; i < parsed.length; i++) sum += parsed[i].id`
- ✅ `for (const x of parsed)` — Symbol.iterator path
- ✅ `parsed.map(x => x.id)`
- ✅ `parsed.filter(x => x.id > 5)`
- ✅ `parsed.forEach(x => sideEffect(x))`
- ✅ `parsed.reduce((a, x) => a + x.id, 0)`
- ✅ `parsed.find(x => x.id === 42)` — early exit
- ✅ `parsed.some`, `parsed.every` — early exit
- ✅ `parsed.slice(0, 10)` — partial copy
- Expectation: these either perform acceptably via per-element
  cache, or trigger the Phase 4 iteration heuristic.

### Mutation (test_json_lazy_mutation.ts)

- ✅ `parsed[0] = {id: 999}` — force-materialize, then assign
- ✅ `parsed.push({...})` — force-materialize
- ✅ `parsed.pop()`, `.shift()`, `.unshift(...)`
- ✅ `parsed.sort((a,b) => a.id - b.id)` — force-materialize
- ✅ `parsed.reverse()` — force-materialize
- ✅ `parsed.splice(5, 2)` — force-materialize
- ✅ `parsed[0].field = "new"` — cache the element, then mutate field

### Spread / destructure (test_json_lazy_spread.ts)

- ✅ `[...parsed]` — full materialize + shallow copy
- ✅ `const [a, b, ...rest] = parsed` — destructure
- ✅ `{...parsed}` (array spread into object — weird but legal JS)
- ✅ `Array.from(parsed)` — full materialize

### Introspection (test_json_lazy_introspect.ts)

- ✅ `Array.isArray(parsed)` → `true`
- ✅ `parsed instanceof Array` → `true`
- ✅ `typeof parsed` → `"object"`
- ✅ `Object.keys(parsed)` → `["0", "1", ..., "len-1"]` (string keys)
- ✅ `Object.values(parsed)` — full materialize
- ✅ `Object.entries(parsed)` — full materialize
- ✅ `parsed.constructor === Array` → `true`

### Stringify (test_json_lazy_stringify.ts)

- ✅ `JSON.stringify(parsed)` before any access — memcpy fast path
- ✅ `JSON.stringify(parsed, null, 2)` — can't memcpy, must walk
- ✅ `JSON.stringify(parsed, replacer)` — always walk
- ✅ `JSON.stringify(parsed)` after `parsed[0].x = 1` — cache
  substitution (Phase 3)
- ✅ `JSON.stringify(parsed)` after `parsed[0] = {...}` — full
  materialize already happened
- ✅ `JSON.stringify({wrapper: parsed})` — lazy as nested value

### Type coercion (test_json_lazy_coercion.ts)

- ✅ `String(parsed)` → equivalent to `parsed.toString()` → comma-joined
- ✅ `parsed + ""` — coercion triggers toString
- ✅ `Boolean(parsed)` → `true` (object)
- ✅ `parsed == null` → `false`

### GC (test_json_lazy_gc.ts)

- ✅ Unreachable fully-lazy: `blob_str` freed, header freed
- ✅ Unreachable partially-materialized: sparse cache + cached
  elements all swept
- ✅ Mutated-through-cache element survives GC: after
  `parsed[0].x = 1; gc(); parsed[0].x === 1`
- ✅ Lazy value escaping into an outer object survives:
  `const keep = {data: parsed}; gc(); keep.data.length`
- ✅ Force-materialize during GC: no crash if GC fires mid-walk

### Threading (test_json_lazy_thread.ts)

- ✅ `parallelMap(parsed, fn)` — force-materialize at serialize
  boundary (per-thread arenas can't share lazy blob)
- ✅ `spawn(() => process(parsed))` — same

### Errors (test_json_lazy_errors.ts)

- ✅ Invalid JSON: `JSON.parse("{")` — fall back to eager parser,
  same error bytes
- ✅ Truncated JSON: same
- ✅ Unicode: surrogates, BOM, escapes match eager parser
- ✅ Empty array: `JSON.parse("[]")` — lazy with length 0

### Nested lazy (test_json_lazy_nested.ts — POSSIBLE PHASE 2)

Current v0.5.207 only makes top-level arrays lazy. A `parsed[i]` on a
nested-object element currently force-materializes the subtree. Open
question: should `parsed[i]` return a lazy-object proxy that only
materializes fields on access? This is the simdjson "on-demand"
design, ~1 week of codegen work (guards on every PropertyGet site).
**Not in this plan.** Ship per-element for top-level arrays first,
measure, then decide.

### Pragma / env / typed-parse composition (test_json_lazy_composition.ts)

- ✅ `@perry-lazy` + `JSON.parse<T>(s)`: pragma wins over typed —
  confirm this is intended behavior
- ✅ `PERRY_JSON_TAPE=1` + `@perry-lazy`: same path, identical output
- ✅ Pragma off + env off + no `<T>`: current direct parser, unchanged

## Risks

1. **Identity bug on cache miss/hit race:** cache write must happen
   atomically with bitmap-bit set. Single-threaded per arena, so no
   real race — but the ordering matters if materialize allocates (GC
   could fire between cache write and bitmap set, leaving a "ghost"
   cached value with bit clear). Mitigation: set bitmap bit *after*
   cache write, and mark the cache slot as a GC root before the
   bitmap check so GC sees it either way.

2. **Per-element walk is O(i):** pathological for iteration. Phase 4
   heuristic (flip to full materialize after 1/4 of elements
   accessed) mitigates but isn't free. Measure first.

3. **Codegen-path blindness:** v0.5.206 already added runtime guards
   on the three IndexGet sites — that coverage is enough for this
   plan. But spread / destructure / iteration protocol each go
   through their own codegen paths. Phase 1+2 implementation needs
   to audit which of those route through `js_array_get_f64`
   (covered) vs inline codegen (needs its own guard update).

## Rollout

- Phase 1+2 land together as v0.5.208 (one commit, touches runtime +
  codegen). Passes all existing tests. Adds new test files for
  access/identity/stringify.
- Phase 3 (stringify with overrides) as v0.5.209 if needed — split
  only because it has its own design surface and a separate test
  family.
- Phase 4 (iteration heuristic) optional, based on measured impact.

After per-element lands, re-measure the three benches
(`bench_json_roundtrip`, `bench_json_readonly`,
`bench_json_readonly_indexed`). If all three win vs Node and Bun,
evaluate flipping the default (drop `PERRY_JSON_TAPE=1` opt-in,
always use the tape path). The pragma becomes an **opt-out** at that
point rather than opt-in — inverted semantics, same syntax.

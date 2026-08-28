# Plan: schema-directed + lazy JSON parsing

> **Archival planning record (2026-04-24).** This is not a current status page
> or source of truth. Validate every unchecked phase against the present JSON
> parser and benchmark suite before resuming work from it.

**Context:** `docs/memory-perf-roadmap.md` tier 1 is landed — Perry beats Node on
both time and RSS on `bench_json_roundtrip` (373 ms / 144 MB vs 385 ms /
188 MB). Bun is still ~1.5× ahead on both (250 ms / 83 MB). The remaining
gap is not fixable by incremental GC tuning — it's architectural. This
doc captures the plan to close it.

**Goal:** beat Bun on both time and peak RSS on `bench_json_roundtrip`
(and ideally on a broader benchmark set) without breaking Node
compatibility.

**Node compatibility is a hard constraint.** At runtime, every change
here must compile down to bytecode indistinguishable from `JSON.parse(…)`
semantics that Node/V8 will execute identically. The `<T>` type
argument is compile-time-only in TypeScript and is fully erased by
`tsc`, so Node never sees it — safe.

## The insight (credit: @Claude in #179 thread)

The benchmark's parse → tree → stringify loop has a **middle
representation** — a 60k-object JSValue tree — that exists only so
arbitrary JS code *could* inspect it. On `bench_json_roundtrip`,
nothing does beyond `parsed.length`. All three of (Bun, Node, simdjson)
accept this middle step as a given. That's the wheel worth reinventing.

Perry is uniquely positioned to attack this because:
- TypeScript types at compile time → know the shape up front
- Compiles to native code → can specialize per call site
- Already does type-directed codegen elsewhere (Buffer intrinsics,
  typed arrays, i32 loop counters)

## Strategy

Four steps, landing in order. Each is independently shippable, each
compounds on the prior.

### Step 1 — Schema-directed parse: `JSON.parse<T>(blob)`

**What:** accept an optional TypeScript type argument on `JSON.parse`.
When the compiler can see T as a concrete object or array type at the
call site, generate a specialized parser path that:
- Pre-builds the expected `keys_array` (no shape-cache lookup per record)
- Allocates objects with the right field count (no overflow map)
- Skips hash-lookup on keys in the expected order (but tolerates
  out-of-order and missing fields — JSON is unordered)
- Uses typed extraction for known leaf types (f64 for number, etc.)

**Compatibility:**
- Runtime: identical to `JSON.parse(blob)` — the `<T>` erases.
- TypeScript: add `parse<T>(text, reviver?): T` overload in Perry's
  ambient types. Projects using stock `lib.es5.d.ts` use
  `JSON.parse(blob) as T` — also erases to the same runtime call.
- Fallback: when the input doesn't match T (extra/missing/wrong-typed
  fields), fall through to the generic parser. Correctness-preserving.

**Design:**

Type descriptor, built at codegen time as static data:
```rust
enum TypeKind { Any, Number, String, Boolean, Null, Object, Array }

struct TypeDescriptor {
    kind: TypeKind,
    // OBJECT: list of expected fields
    fields: *const FieldDescriptor,
    field_count: u32,
    // ARRAY: element type
    element: *const TypeDescriptor,
}

struct FieldDescriptor {
    name_ptr: *const u8,
    name_len: u32,
    type_ptr: *const TypeDescriptor,
}
```

Call-site codegen emits one static descriptor per distinct type shape,
dedup-ed at module scope. The static descriptor lives in `.rodata` —
zero runtime cost to build.

Runtime entry: `js_json_parse_typed(blob, *const TypeDescriptor) -> JSValue`.
Walks the descriptor tree as it parses; on shape miss, tail-calls
`js_json_parse` with the same input.

**Expected win on `bench_json_roundtrip`:** ~20-40% parse speedup.
Not enough to beat Bun alone — sets up infrastructure for Step 2.

**Scope:**
- `crates/perry-hir`: recognize `JSON.parse<T>` in the type-argument
  position; carry T through to HIR.
- `crates/perry-codegen`: emit static type descriptors + routed call.
- `crates/perry-runtime/src/json.rs`: new `js_json_parse_typed`.
- Types overlay: add `parse<T>(…): T` overload.

**Tests:** new `test-files/test_json_typed_*.ts` — must not touch existing
`test_json_*.ts` files. Parity test against Node's `JSON.parse(blob) as T`
semantics (identical, since the type is erased).

**Benchmarks:** new `benchmarks/suite/bench_json_typed_roundtrip.ts` —
mirrors `bench_json_roundtrip` but adds `<Item[]>` type argument.
Side-by-side measurement keeps both benches, shows the delta.

### Step 2 — Tape-based lazy parse

**What:** replace `DirectParser::parse_value` with a two-phase design:

*Phase 1 — tape build.* One SIMD-friendly pass over the blob, emits a
flat `Tape` of `(offset, kind)` structural positions. No tree, no
JSValue allocation, no strings. Output size bounded by input size. On
bench_json_roundtrip's 1 MB blob, the tape is ~100 KB of u32s.

*Phase 2 — lazy materialization.* `JSON.parse` returns a `TapedJsValue`
— a small handle (tape pointer, root position, blob pointer) wrapped in
a JSValue. Property access, iteration, and `.length` read from the tape
and decode on demand. Full materialization only happens if the user:
- Mutates a field (`parsed.x = 5`) → materialize the enclosing object
  and all ancestors to the root, then switch to tree-mode for that path
- Passes the value to an FFI boundary that reads bytes opaquely
- Calls a method that walks structurally (Object.keys, for…in)

Subtrees never touched stay as tape views forever. On
`bench_json_roundtrip` where only `.length` is read, 99% of the tree
never materializes.

**Stringify pairing:** if a `TapedJsValue` hasn't been mutated, stringify
is a memcpy of the relevant blob bytes (with re-escaping handled by the
tape kinds). Zero tree walk. If mutated, fall through to the generic
stringifier.

**Compatibility:**
- Runtime: `JSON.parse(blob)` returns a value that behaves indistinguishably
  from the current tree for all observable operations (property access,
  iteration, stringify). Performance characteristics differ.
- No source-code changes required — purely a runtime refactor.
- All existing JSON tests should pass unchanged.

**Design challenges:**
- NaN-boxing representation for `TapedJsValue` (new pointer type vs.
  reuse POINTER_TAG with a discriminant flag in the header)
- Proxy semantics: property access on tape views dispatches through a
  helper that walks the tape; must maintain pointer identity where
  required (`a === a` must hold for the same tape path)
- Write barrier: mutation triggers materialization — needs to propagate
  the new tree pointer back through all referring JSValues

**Expected win on `bench_json_roundtrip`:** RSS ≤50 MB (below Bun's 83 MB),
time ≤150 ms (below Bun's 250 ms).

**Scope:**
- `crates/perry-runtime/src/json.rs`: tape types, tape builder
  (SIMD structural scan), taped parser entry
- `crates/perry-runtime/src/value.rs`: new tag or flag for
  taped values, `is_taped` predicate
- `crates/perry-runtime`: object/array accessors route through
  `materialize_if_taped` helper
- Every existing is_object/is_array consumer audited for taped-view
  semantics

**Tests:** new `test-files/test_json_lazy_*.ts` exercising
pure-read, single-mutation, full-materialization paths. All existing
`test_json_*.ts` must continue passing byte-for-byte against Node.

**Benchmarks:**
- `benchmarks/suite/bench_json_lazy_readonly.ts` — read `.length` only,
  never touch fields. Expected: ~10× faster, ~20× less RSS than current.
- `benchmarks/suite/bench_json_lazy_full.ts` — touch every field,
  forcing full materialization. Expected: at most 2× slower than current
  (the tape-then-materialize overhead).

### Step 3 — Generational GC (per memory-perf-roadmap tier 3)

**What:** young nursery + old space. Precise root tracking via codegen
shadow stacks. Non-moving within a generation.

**Why still:** Step 1 + Step 2 solve the JSON case. Other workloads that
allocate short-lived temporaries (array comprehensions, string
building, iterator chains) still get Perry's current flat-arena
treatment. Generational GC closes the Bun gap for them too.

**Scope / risk:** 3-4 weeks. See `docs/memory-perf-roadmap.md` tier 3 #6.

### Step 4 — Mutation-tracking round-trip (speculative)

**What:** when a taped value is mutated, record the mutation in a
path-indexed overlay instead of materializing. On stringify, emit
un-mutated ranges as memcpy from the blob, emit mutated paths through
the generic stringifier, splice.

**Expected win:** 100× on the `parse → mutate-1% → stringify` pattern.

**Parked:** too speculative until Step 2 proves out. The tape
infrastructure is a prerequisite.

## Order of operations & testing gates

1. Step 1a: `JSON.parse<T>` signature + codegen pass-through (no fast
   path yet — just carry the type argument end-to-end without
   regressing anything)
2. Step 1b: `js_json_parse_typed` with pre-built shape for top-level
   object. Add tests, benchmarks. Measure.
3. Step 1c: extend to `Array<T>` and nested object types.
4. **Gate:** schema-directed parse working on the new typed benchmark.
   Ship as a minor version bump.
5. Step 2a: tape types + builder (no consumer yet). Unit tests for the
   builder itself.
6. Step 2b: taped parser entry behind a feature flag
   (`PERRY_LAZY_JSON=1`). Both paths coexist.
7. Step 2c: flip flag default to on after full regression sweep
   (`test_json_*`, all gap tests, all regression benches).
8. **Gate:** `bench_json_roundtrip` RSS below Bun's. Time at most
   1.2× Bun's. Ship as a major version bump.
9. Step 3 planning follows the roadmap.

## Invariants (must hold across all steps)

- `JSON.parse(blob)` with no type argument behaves identically to
  today — byte-for-byte compatible with Node for all existing inputs.
- `JSON.parse<T>(blob)` at runtime is identical to `JSON.parse(blob) as T`.
  TypeScript erases the `<T>`; Perry's compiler may use it for
  specialization, but never for semantic change.
- Mutation semantics of parse output are identical between
  (tape-backed) and (tree-backed) implementations. Including `===`
  identity on repeated property access within the same expression.
- All 28 `test_gap_*` tests at 24/28 or better throughout.
- `07_object_create`, `12_binary_trees`, `02_loop_overhead`,
  `06_math_intensive`, `bench_gc_pressure`, `bench_array_grow`
  within 5% of v0.5.198 baseline for every intermediate commit.

## New test coverage

**Step 1 tests** (all new files, no conflict with existing
`test_json_*.ts`):
- `test-files/test_json_typed_basic.ts` — `JSON.parse<{a: number, b: string}>`
  on exact-shape input
- `test-files/test_json_typed_extra_fields.ts` — input has fields not
  in T; should be tolerated (present in result)
- `test-files/test_json_typed_missing_fields.ts` — T declares more
  fields than input; missing ones → undefined
- `test-files/test_json_typed_nested.ts` — nested object and array shapes
- `test-files/test_json_typed_array.ts` — `JSON.parse<Item[]>`
- `test-files/test_json_typed_mismatch.ts` — wrong runtime type for a
  field (e.g. string in number slot); must fall through to generic
  parser without crashing

**Step 2 tests** (new):
- `test-files/test_json_lazy_readonly.ts` — read one field, compare to
  Node byte-for-byte
- `test-files/test_json_lazy_identity.ts` — `const a = p.x; const b = p.x;
  assert a === b` (pointer identity on repeated access)
- `test-files/test_json_lazy_mutate.ts` — mutate one field, then verify
  ancestor chain is materialized correctly
- `test-files/test_json_lazy_stringify.ts` — parse then immediate
  stringify must produce the same bytes (modulo JSON normalization)

## New benchmarks

- `benchmarks/suite/bench_json_typed_roundtrip.ts` — mirror of
  `bench_json_roundtrip` with `<Item[]>` type argument
- `benchmarks/suite/bench_json_lazy_readonly.ts` — parse + read `.length`
  only, 50 iters × 1 MB blob
- `benchmarks/suite/bench_json_lazy_full.ts` — parse + touch every field,
  50 iters × 1 MB blob

Each benchmark compared against Node and Bun for the same workload.

## Log

| Date | Version | Change | Result |
|---|---|---|---|
| 2026-04-24 | v0.5.198 | Tier 1 complete (roadmap) | 373 ms / 144 MB — beats Node, behind Bun |
| TBD | | Step 1a: `JSON.parse<T>` passthrough | no perf change, plumbing only |
| TBD | | Step 1b: typed parse with pre-built shape | |
| TBD | | Step 2: tape-based lazy parse | |

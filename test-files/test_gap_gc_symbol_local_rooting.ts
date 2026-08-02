// #7236: a `Symbol`-typed local names a GC-heap object, and needs a
// shadow-stack slot like any other heap reference.
//
// `collectors/pointer_locals.rs` listed `Type::Symbol` in
// `is_definitely_non_pointer_type`, so `const s = Symbol("w")` got NO
// shadow-stack slot and lived in a plain `alloca` for its whole scope —
// invisible to the precise root walk. `typed_shape::type_is_pointer_bearing`,
// which lays out the GC's own field masks, said `Symbol` IS a pointer, so the
// two copies of one question had drifted by exactly one variant.
//
// ★ WHAT ACTUALLY GOES WRONG IS A PREMATURE FREE, NOT A STALE ADDRESS.
// `alloc_symbol` is `gc_malloc(size_of::<SymbolHeader>(), GC_TYPE_STRING)`, and
// `gc_malloc` (gc/malloc.rs) is the SYSTEM allocator with a `GcHeader` in front
// — not an arena allocation. So a fresh symbol is not in the nursery and the
// copying minor cannot relocate it; what it can do is `dealloc` it. Under the
// #7235 taxonomy the local is RECLAIMABLE-but-not-MOVABLE, the #7230 class.
// `alloc_symbol`'s own comment concedes the liveness half — a fresh symbol is
// kept alive "through the SYMBOL_REGISTRY (for registered symbols) or NOT AT
// ALL" — and `SYMBOL_POINTERS` does not save it either:
// `scan_symbol_pointer_metadata_roots_mut` visits it with
// `visit_metadata_usize_slot`, which rewrites a recorded address WITHOUT
// marking. With no shadow slot the unrooted `alloca` is the only reference in
// the world, the collector cannot see it, and `sweep_malloc_objects` frees a
// live symbol.
//
// ★ WHY THE PRESSURE IS SYMBOLS AND NOT OBJECTS. The malloc sweep inside the
// copying minor is gated: `copied_minor_malloc_sweep_due` is true only for a
// `GcTriggerKind::MallocCount` collection or once `malloc_object_count()`
// passes its trigger. Driving pressure with object/array churn reaches the
// ARENA trigger instead, so the malloc sweep is only occasionally due and the
// defect reproduces intermittently — measured at 1 failure in 80 probes, and at
// 0 on four of five repeats of another shape. Allocating symbols is what makes
// the malloc-count trigger the one that fires, and with it the reproduction
// deterministic. It also makes the reuse deterministic: the freed block is
// exactly the size class the next `Symbol()` asks for, so the recycled bytes
// are what the stale local reads back.
//
// Measured on `f8f1e7188` (before the fix), `--release`: `A 34` on the
// `loop_polls` arm, identical on three consecutive runs, and `A 34` on the
// SHIPPED DEFAULT with no GC env at all. Oracle and fixed build both print
// `A 0`. This one does not need a moving arm to bite — the conservative stack
// scan is what has been hiding it, and it stops hiding it as soon as the
// collection is malloc-count driven.
//
// ★ WHY THE PROBES ARE NOT IDENTITY COMPARISONS. `js_symbol_equals`
// (symbol/constructors.rs) falls back from a bits comparison to dereferencing
// both headers and comparing `id`, and `js_is_symbol` falls back to reading
// `magic` — both answer correctly off a freed-but-not-yet-recycled header, so
// an identity probe reports nothing. What a reaped symbol cannot survive is
// having its own description read back after its storage is reused.
//
// The observable is byte-for-byte identical to `node --experimental-strip-types`.

function symChurn(n: number): number {
  let k = 0;
  for (let i = 0; i < n; i++) {
    const t = Symbol("t");
    if (typeof t === "symbol") {
      k++;
    }
  }
  return k;
}

// A: the symbol is referenced by NOTHING but the local across the collection,
// and is then asked what it is.
function heldAcrossCollection(): number {
  let bad = 0;
  for (let r = 0; r < 20; r++) {
    const s = Symbol("w");
    if (symChurn(50000) !== 50000) {
      bad++;
    }
    if (typeof s !== "symbol") {
      bad++;
    }
    if (String(s) !== "Symbol(w)") {
      bad++;
    }
    if (s.description !== "w") {
      bad++;
    }
  }
  return bad;
}

// B: the same local, then used as a COMPUTED KEY — the issue's other named
// shape. The store must land on a symbol key whose description round-trips, so
// a local whose storage was recycled into one of `symChurn`'s throwaway
// `Symbol("t")`s is caught by the description rather than by the count.
function usedAsComputedKey(): number {
  let bad = 0;
  for (let r = 0; r < 20; r++) {
    const s = Symbol("k");
    if (symChurn(50000) !== 50000) {
      bad++;
    }
    const o: any = {};
    o[s] = r;
    if (o[s] !== r) {
      bad++;
    }
    const keys = Object.getOwnPropertySymbols(o);
    if (keys.length !== 1) {
      bad++;
    }
    if (String(keys[0]) !== "Symbol(k)") {
      bad++;
    }
  }
  return bad;
}

console.log("A", heldAcrossCollection());
console.log("B", usedAsComputedKey());

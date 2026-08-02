// #7236: a `Symbol`-typed local names a GC-heap object, and needs a
// shadow-stack slot like any other heap reference.
//
// `collectors/pointer_locals.rs` listed `Type::Symbol` in
// `is_definitely_non_pointer_type`, so `const s = Symbol("k")` got NO
// shadow-stack slot and lived in a plain `alloca` for its whole scope —
// invisible to the precise root walk. `typed_shape::type_is_pointer_bearing`,
// which lays out the GC's own field masks, said `Symbol` IS a pointer, so the
// two copies of one question had drifted by exactly one variant.
//
// ★ THE FAILURE IS A PREMATURE FREE, NOT A STALE ADDRESS. `alloc_symbol` is
// `gc_malloc(size_of::<SymbolHeader>(), GC_TYPE_STRING)`, and `gc_malloc`
// (gc/malloc.rs) is the SYSTEM allocator with a `GcHeader` in front — not an
// arena allocation. So a fresh symbol is not in the nursery and the copying
// minor cannot relocate it; what it can do is `dealloc` it, via
// `sweep_malloc_objects`. Under #7235's taxonomy the local is
// RECLAIMABLE-but-not-MOVABLE, the #7230 class. Nothing else holds a fresh
// symbol — `alloc_symbol`'s own comment says it is kept alive "through the
// SYMBOL_REGISTRY … or NOT AT ALL", and `scan_symbol_pointer_metadata_roots_mut`
// visits `SYMBOL_POINTERS` with `visit_metadata_usize_slot`, which rewrites a
// recorded address WITHOUT marking. With no shadow slot the `alloca` is the
// only reference in the world and a live symbol is freed.
//
// ★ WHY THE PRESSURE IS SYMBOLS AND NOT OBJECTS. The malloc sweep inside the
// copying minor is gated: `copied_minor_malloc_sweep_due` is true only for a
// `GcTriggerKind::MallocCount` collection or once `malloc_object_count()`
// passes its trigger. Object/array churn reaches the ARENA trigger instead, so
// the defect reproduces intermittently — measured at 1 failure in 80 probes on
// one object-churn shape, and at 0 on four of five repeats of another after a
// first run showed 72. Symbol churn makes the malloc-count trigger the one that
// fires, and makes the reuse deterministic too: the freed block is exactly the
// size class the next `Symbol()` asks for, so the recycled bytes are one of
// `symChurn`'s throwaway `Symbol("t")`s and the stale local reads ITS
// description back.
//
// ★ WHY THE PROBE IS NOT AN IDENTITY COMPARISON. `js_symbol_equals`
// (symbol/constructors.rs) falls back from a bits comparison to dereferencing
// both headers and comparing `id`, and `js_is_symbol` falls back to reading
// `magic` — and the block that recycles a freed symbol is ANOTHER SYMBOL, so
// both answer "yes, a symbol" and `typeof` never moves (measured: `typeof`
// 0/20 at base). What a reaped symbol cannot survive is being used as a
// computed KEY and then having that key read back and identified, which is
// #7236's own named shape.
//
// ★ WHY THE PROBE SYMBOL HAS NO DESCRIPTION. `Symbol()` rather than
// `Symbol("k")`, on purpose, to keep this file gating ONE defect.
// `GC_TYPE_STRING`'s type-info entry is `pointer_free: true` /
// `GcRewriteDescriptorKind::Leaf` — `alloc_symbol` says so in as many words
// ("the GC won't try to scan internal pointers") — so a symbol's
// `description` `StringHeader*` is never traced or rewritten, and a symbol
// that is itself perfectly rooted can still have its description reaped out
// from under it. That is a SEPARATE, runtime-side defect (#7246)
// that this codegen change does not touch, and with `Symbol("k")` it left this
// file red 2/20 on
// the `PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off` arms even
// after the fix. A descriptionless symbol has no such pointer, so what remains
// is exactly the lifetime of the symbol object — which is what a shadow slot
// decides. The recycled block is one of `symChurn`'s `Symbol("t")`s, so the
// stale local reports `Symbol(t)` where the oracle says `Symbol()`.
//
// Measured at base (`f8f1e7188`), release, oracle node 26.5.1 (`B 0`):
// `B 34` of a possible 80, 3/3 identical, on `loop_polls` and on the shipped
// default; `B 0` after the fix, 3/3.
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

function usedAsComputedKey(): number {
  let bad = 0;
  for (let r = 0; r < 20; r++) {
    const s = Symbol();
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
    if (String(keys[0]) !== "Symbol()") {
      bad++;
    }
    if (keys[0].description !== undefined) {
      bad++;
    }
  }
  return bad;
}

console.log("B", usedAsComputedKey());

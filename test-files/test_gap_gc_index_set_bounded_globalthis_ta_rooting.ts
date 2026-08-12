// #7640 section A: three `index_set.rs` arms that lowered a receiver (and
// sometimes a key) with no rooting decision at all, all fixed in the same
// change as this test.
//
// 1. The bounded-index-pair array store: the receiver was lowered, then the
//    (proven, non-allocating) loop-counter index, then the VALUE — and
//    `classify_for_length_hoist`'s body predicate accepts an allocating RHS
//    (`Expr::Object`) at exactly this call site, so `arr[i] = {v:i}` inside
//    the loop this fact comes from left the receiver a bare register across
//    the object literal's allocation.
// 2. `globalThis[k] = v`: the key (a heap string) and the globalThis
//    singleton copy were both live across the allocating RHS.
// 3. The width-tracked typed-array non-numeric-index store (`ta[k] = mk()`
//    with `k: any`): receiver and key both live across the allocating RHS —
//    here the key is a `Symbol`, which allocates when interned.

function churn(seed: number): number {
  const bits: unknown[] = [];
  for (let i = 0; i < 500; i++) {
    bits.push({ i: i, s: "w" + i });
  }
  return seed + bits.length - 500;
}

function makeRecord(n: number): { v: number } {
  return { v: churn(n) };
}

function boundedIndexStore(n: number): unknown[] {
  // Matches the issue's own reproducer shape verbatim: a `let`-bound LOCAL
  // array (`classify_for_length_hoist`'s bounded-pair registration is keyed
  // on that shape) and — crucially — the store's RHS is an OBJECT LITERAL
  // (`Expr::Object`) at the syntax level, which is exactly what
  // `expr_preserves_array_length`'s body predicate accepts. A RHS that is a
  // plain function call (`a[i] = makeRecord(n)`) does not classify the same
  // way and falls through to the already-protected generic array-store arm
  // instead of the bounded-index-pair one this test exists to exercise.
  const a: unknown[] = new Array(64);
  for (let i = 0; i < a.length; i++) {
    a[i] = { v: churn(n + i) };
  }
  return a;
}

function globalThisStore(key: string, n: number): void {
  (globalThis as unknown as Record<string, unknown>)[key] = makeRecord(n);
}

function widthTrackedTaSymbolStore(ta: Int32Array, n: number): void {
  const sym = Symbol.for("gc7640a_" + n);
  (ta as unknown as Record<symbol, number>)[sym] = churn(n);
}

function main(): void {
  let bad = 0;

  for (let r = 0; r < 200; r++) {
    const a = boundedIndexStore(r);
    for (let i = 0; i < a.length; i++) {
      const rec = a[i] as { v: number };
      if (rec.v !== r + i) bad++;
    }
  }

  for (let r = 0; r < 200; r++) {
    const key = "gc7640a_global_" + (r % 7);
    globalThisStore(key, r);
    const rec = (globalThis as unknown as Record<string, { v: number }>)[key];
    if (rec.v !== r) bad++;
  }

  const ta = new Int32Array(8);
  for (let r = 0; r < 200; r++) {
    widthTrackedTaSymbolStore(ta, r);
  }

  console.log(bad);
}

main();

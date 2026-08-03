// #7302 GC × exception-transport probe: allocate inside a `try`, throw
// ACROSS a collection point, and verify both the caught value and the
// locals that were live in the catching frame. No such probe existed, which
// is exactly why the statepoint experiment's unsound `has_try` fallback went
// unnoticed (#7174).
//
// Three pressure points:
//   1. The thrown object is allocated after heavy churn, so it is young when
//      the unwind happens (a mover would relocate it; a sweep bug frees it).
//   2. The frames being unwound past hold live allocations of their own —
//      their shadow frames are dropped by the savepoint restore, and the
//      catching frame's locals must SURVIVE (they are roots of the catcher,
//      not of the unwound callees).
//   3. The catch body churns again before reading anything, so a stale
//      pointer cannot masquerade as correct.

function churn(n: number): number {
  const a: any[] = [];
  for (let i = 0; i < n; i++) {
    a.push({ i: i, s: "c" + (i & 7) });
  }
  return a.length;
}

function thrower(depth: number, tag: number): number {
  // Live allocation in every frame the unwind will discard.
  const mine = { tag: tag, depth: depth, pad: "x" + depth };
  if (depth === 0) {
    churn(400);
    const err: any = new Error("gc-throw-" + tag);
    err.payload = { tag: tag, arr: [tag, tag + 1, tag + 2] };
    throw err;
  }
  const r = thrower(depth - 1, tag) + mine.depth;
  return r;
}

function run(): string {
  let bad = 0;
  for (let r = 0; r < 200; r++) {
    // Locals of the CATCHING frame, allocated before the try — these must
    // survive the collection triggered on the throw path.
    const keeper = { r: r, name: "keeper" + r, list: [r, r * 2, r * 3] };
    const keeperStr = "s" + r;
    try {
      churn(300);
      thrower(40, r);
      bad += 1000; // unreachable
    } catch (e: any) {
      churn(400);
      if (e.message !== "gc-throw-" + r) bad++;
      if (e.payload.tag !== r) bad++;
      if (e.payload.arr[2] !== r + 2) bad++;
      if (keeper.name !== "keeper" + r) bad++;
      if (keeper.list[1] !== r * 2) bad++;
      if (keeperStr !== "s" + r) bad++;
    }
  }
  return bad === 0 ? "ok" : "BAD:" + bad;
}

console.log(run());

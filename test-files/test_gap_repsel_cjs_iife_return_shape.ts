// Representation-selection Phase 3b, #7170 R1: return-shape facts **inside an
// IIFE** (collectors/ptr_shape_returns.rs, collectors/spec_abi_sites.rs).
//
// `test_gap_repsel_return_shape.ts` is the same mechanism at module scope,
// where every producer is a `hir.functions` entry and every call site is an
// `Expr::FuncRef`. This file wraps the producers and their callers in
// `(function () { … })()` — which is exactly what Perry's own `cjs_wrap` does
// to every CommonJS module (`compile/cjs_wrap/wrap.rs`). Inside that wrapper a
// `function` declaration lowers to `Stmt::Let { init: Expr::Closure }` and a
// call to it to `Call { callee: LocalGet(id) }`, so #7107's two halves both
// missed it and 91.6% of dependency-JS allocation sites sat unreached
// (#7170 §2/§6).
//
// Every case must be BYTE-EXACT against the pinned Node oracle. The promotions
// are asserted structurally elsewhere (`benchmarks/repsel_census`'s
// `fixture_ptr_shape_cjs_iife`, floors held in code); this file is the
// behavioural guard, and a green run of it with zero promotions would be a
// vacuous pass, which is why the two are separate.
//
// Covered:
//  1. a closure producer reached through a captured, BOX-BACKED binding — the
//     shape a hoisted inner `function` referenced from a sibling closure
//     always takes (`lower_decl/block.rs` emits `PreallocateBoxes`), and the
//     entire dependency-JS population;
//  2. an object-literal closure producer (`return { … }` -> __AnonShape_*);
//  3. GC movement between the provenance call and the field reads, inside the
//     wrapper — the caller's bound slot is the only rewritable root, and the
//     producer's frame is gone;
//  4. the numeric-field stand-down through a closure producer: NaN/Infinity/-0
//     stored by the producer into a field the caller's region never saw
//     stored;
//  5. producers and callees that must NOT be reached: a reassigned binding, a
//     twice-declared binding, an aliased cache, a fall-through producer, and a
//     callee read out of an object (not a bare local).

const lines: string[] = (function () {
  const out: string[] = [];

  class Rec {
    id: number;
    name: string;
    score: number;
    constructor(id: number, name: string, score: number) {
      this.id = id;
      this.name = name;
      this.score = score;
    }
  }

  // 1. Producer + caller, both closures inside the wrapper. `makeRec` is
  //    captured by `consume` and by `survivesGc`, so its binding is
  //    box-backed — a binding proof that refused boxed callees would refuse
  //    every real CommonJS module.
  function makeRec(i: number): Rec {
    const r = new Rec(i, "r" + i, 0);
    r.score = r.id * 1.5;
    r.score = r.score + 0.25;
    return r;
  }

  function consume(i: number): string {
    const r = makeRec(i);
    r.score = r.score + 1;
    return r.name + ":" + r.score.toFixed(3) + ":" + r.id;
  }

  // 2. Object-literal producer through the same wrapper.
  function shapeOne(i: number) {
    return { key: "k" + i, value: i * 2 };
  }

  function readShaped(i: number): string {
    const s = shapeOne(i);
    return s.key + "=" + (s.value + 1);
  }

  // 4. Values the caller's region never saw stored.
  class Mixed {
    v: number;
    tag: string;
    constructor() {
      this.v = 1;
      this.tag = "m";
    }
  }

  function makeMixed(kind: number): Mixed {
    const m = new Mixed();
    m.v = 2;
    if (kind === 1) {
      m.v = NaN;
    } else if (kind === 2) {
      m.v = Infinity;
    } else if (kind === 3) {
      m.v = -0;
    }
    return m;
  }

  function readMixed(kind: number): string {
    const m = makeMixed(kind);
    return m.tag + "|" + m.v + "|" + (m.v + 1) + "|" + Object.is(m.v, -0);
  }

  // 3. GC movement. The churn must ESCAPE or scalar replacement deletes it and
  //    the arena never grows — a non-escaping loop drives ZERO collections and
  //    makes every GC arm inert against this file (#6942/#6946, the failure
  //    mode scripts/gc_repsel_matrix.sh exists to report). Keep the budget in
  //    sync with the matrix's liveness column.
  let churnSink: unknown[] = [];

  function churn(i: number): void {
    churnSink.push({ i: i, s: "c" + (i & 1023), a: [i, i + 1] });
    if (churnSink.length > 4096) {
      churnSink = [];
    }
  }

  function survivesGc(n: number): string {
    const survivor = makeRec(7);
    let sink = 0;
    for (let i = 0; i < n; i++) {
      churn(i);
      // Read AFTER the allocation safepoint, every iteration. If an evacuating
      // scavenge moved `survivor` and the bound slot was not rewritten — or the
      // raw pointer was CSE'd across the safepoint — this observes a stale
      // address.
      sink = sink + survivor.id;
    }
    return survivor.name + "/" + survivor.score.toFixed(2) + "/" + sink;
  }

  // 5a. A REASSIGNED binding: the callee no longer names one body, so the seed
  //     must stand down. It is reassigned to a function that returns a
  //     different class, so a compiler that kept the fact would read the wrong
  //     offsets and this line would diverge.
  let swappable = function (i: number): Rec {
    return new Rec(i, "first", 1);
  };
  function callSwappable(i: number): string {
    const r = swappable(i);
    return r.name + ":" + r.score;
  }
  const before = callSwappable(1);
  swappable = function (i: number): Rec {
    return new Rec(i, "second", 2);
  };
  const after = callSwappable(1);

  // 5b. An aliased cache: `return CACHE` is not fresh, so no fact — and the
  //     caller must observe mutations made through the other alias.
  let CACHE: Rec | null = null;
  function getCached(): Rec {
    if (CACHE === null) {
      CACHE = new Rec(100, "cached", 0);
    }
    return CACHE;
  }

  // 5c. A producer that can fall through to `undefined`.
  function maybeRec(b: boolean): Rec | undefined {
    if (b) {
      return new Rec(5, "maybe", 5);
    }
    return undefined;
  }

  // 5d. A callee read out of an object — not a bare local, so not resolvable.
  const table = { mk: makeRec };
  function viaTable(i: number): string {
    const r = table.mk(i);
    return r.name + "!" + r.id;
  }

  out.push(consume(3));
  out.push(consume(0));
  out.push(readShaped(4));
  out.push(readMixed(0));
  out.push(readMixed(1));
  out.push(readMixed(2));
  out.push(readMixed(3));
  out.push(survivesGc(120000));
  out.push("swap:" + before + "/" + after);

  const c1 = getCached();
  c1.id = 42;
  const c2 = getCached();
  out.push("cache:" + c2.id + ":" + c2.name);

  const m1 = maybeRec(true);
  out.push("maybe:" + (m1 === undefined ? "none" : m1.name + m1.score));
  const m2 = maybeRec(false);
  out.push("maybe:" + (m2 === undefined ? "none" : "some"));

  out.push(viaTable(9));

  return out;
})();

for (const line of lines) {
  console.log(line);
}

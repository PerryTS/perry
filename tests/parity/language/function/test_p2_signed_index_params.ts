// P2 typed signatures: a method whose parameters feed element keys is typed
// when the facts give them I32, whatever their sign (an int32 key is bounded
// by one unsigned compare). Exact against node with the rule on, off
// (PERRY_SIG_SIGNED_INDEX=0) and in check mode.
// A loop bound the compiler cannot fold keeps each loop as written.
const RUN = Math.random() < 2 ? 1 : 0;

// The shape: a board helper that reads the old element, stores the new one
// and updates two hashes through keys built from both parameters.
class Board {
  cells: number[] = new Array<number>(64).fill(0);
  za: number[] = [];
  zb: number[] = [];
  ha = 0;
  hb = 0;
  constructor() {
    let seed = 12345;
    for (let i = 0; i < 16 * 64; i++) {
      seed = (seed * 1103515245 + 12345) | 0;
      this.za.push(seed);
      seed = (seed * 1103515245 + 12345) | 0;
      this.zb.push(seed);
    }
  }
  at(square: number): number {
    return this.cells[square] ?? 0;
  }
  set(square: number, piece: number): void {
    const old = this.at(square);
    if (old !== 0) {
      this.ha ^= this.za[(old + 6) * 64 + square] ?? 0;
      this.hb ^= this.zb[(old + 6) * 64 + square] ?? 0;
    }
    this.cells[square] = piece;
    if (piece !== 0) {
      this.ha ^= this.za[(piece + 6) * 64 + square] ?? 0;
      this.hb ^= this.zb[(piece + 6) * 64 + square] ?? 0;
    }
  }
}

// 1. In-range keys and pieces of both signs: the typed body, every site.
const b1 = new Board();
for (let i = 0; i < 64 * RUN; i++) b1.set(i, (i % 13) - 6);
for (let i = 0; i < 64 * RUN; i += 3) b1.set(i, 0);
for (let i = 0; i < 64 * RUN; i += 5) b1.set(i, i & 1 ? -3 : 4);
let sum1 = 0;
for (let i = 0; i < 64 * RUN; i++) sum1 += b1.at(i) * (i + 1);
console.log("in-range", sum1, b1.ha, b1.hb, b1.cells.length);

// 2. A negative key beside in-range ones: it names a property, no element,
// and the old value read back through `at` is that property's.
const b2 = new Board();
for (let i = 0; i < 32 * RUN; i++) b2.set(i - 8, (i % 5) - 2);
console.log(
  "negative key",
  b2.cells.length,
  b2.cells[0],
  b2.cells[23],
  (b2.cells as any)[-8],
  (b2.cells as any)[-1],
  Object.keys(b2.cells).length,
  b2.ha,
  b2.hb,
);
b2.set(-8, 5);
b2.set(-8, -5);
console.log("negative key again", (b2.cells as any)[-8], b2.at(-8), b2.ha, b2.hb);

// 3. A key past the length (inside the reserved hull, then far past it).
const b3 = new Board();
for (let i = 0; i < 70 * RUN; i++) b3.set(i, 1 + (i & 3));
console.log("past the length", b3.cells.length, b3.cells[69], b3.at(69), b3.at(70), b3.ha, b3.hb);
const b3b = new Board();
for (let i = 0; i < 4 * RUN; i++) b3b.set(i * 100, 2);
console.log("far past", b3b.cells.length, b3b.cells[300], b3b.cells[299], b3b.at(301), b3b.ha, b3b.hb);

// 4. Negative controls on the value: a fraction, -0, NaN, undefined. The
// parameter is no I32, so the method keeps its index clones (or a double
// slot), and the output is still exact.
class Loose {
  cells: number[] = new Array<number>(16).fill(0);
  h = 0;
  set(square: number, piece: number): void {
    const old = this.cells[square] ?? 0;
    if (old !== 0) this.h = (this.h + old * 3 + square) | 0;
    this.cells[square] = piece;
    if (piece !== 0) this.h = (this.h ^ (((piece + 6) * 16 + square) | 0)) | 0;
  }
}
const l1 = new Loose();
for (let i = 0; i < 16 * RUN; i++) l1.set(i, i - 4);
l1.set(3, 1.5);
l1.set(4, -0);
l1.set(5, NaN);
l1.set(6, undefined as any);
console.log(
  "loose values",
  l1.cells[3],
  Object.is(l1.cells[4], -0),
  Number.isNaN(l1.cells[5]),
  l1.cells[6],
  l1.h,
  l1.cells.join(","),
);

// 5. Negative controls on the key: a fraction, a numeric string, NaN,
// undefined, -0. Each names a property (or element 0 for -0).
class LooseKey {
  cells: number[] = new Array<number>(16).fill(0);
  n = 0;
  put(square: number, piece: number): void {
    const old = this.cells[square] ?? 0;
    if (old !== 0) this.n++;
    this.cells[square] = piece;
  }
}
const k1 = new LooseKey();
for (let i = 0; i < 16 * RUN; i++) k1.put(i, i + 1);
k1.put(1.5, 7);
k1.put("3" as any, 8);
k1.put(NaN, 9);
k1.put(undefined as any, 10);
k1.put(-0, 11);
k1.put(-1, 12);
console.log(
  "loose keys",
  k1.cells.length,
  k1.cells[0],
  k1.cells[3],
  (k1.cells as any)[1.5],
  (k1.cells as any)["NaN"],
  (k1.cells as any)["undefined"],
  (k1.cells as any)[-1],
  k1.n,
  Object.keys(k1.cells).length,
);

// 6. The method retained and called indirectly: extracted, `call`, `apply`,
// `bind`, and through an array of functions. These enter the boxed entry
// (or keep the method untyped); in-range direct sites stand beside them.
class Kept {
  cells: number[] = new Array<number>(32).fill(0);
  h = 0;
  set(square: number, piece: number): void {
    const old = this.cells[square] ?? 0;
    if (old !== 0) this.h = (this.h - old) | 0;
    this.cells[square] = piece;
    if (piece !== 0) this.h = (this.h + (piece + 6) * 32 + square) | 0;
  }
}
const kept = new Kept();
for (let i = 0; i < 32 * RUN; i++) kept.set(i, (i % 7) - 3);
const loose = kept.set;
loose.call(kept, 2, -6);
loose.apply(kept, [3, 6]);
const bound = kept.set.bind(kept);
bound(4, -1);
const fns = [bound, (s: number, p: number) => kept.set(s, p)];
for (let i = 0; i < 8 * RUN; i++) fns[i & 1](5 + i, i - 4);
loose.call(kept, -2, 9);
loose.call(kept, 2.5, 9);
loose.call(kept, 6, 0.25);
const other = { cells: [1, 2, 3], h: 100 };
loose.call(other as any, 1, 5);
console.log(
  "kept",
  kept.h,
  kept.cells.join(","),
  (kept.cells as any)[-2],
  (kept.cells as any)[2.5],
  other.cells.join(","),
  other.h,
);

// 7. A subclass that overrides the helper: the site has two bodies.
class Sub extends Kept {
  set(square: number, piece: number): void {
    this.cells[square] = piece * 2;
  }
}
const mixed: Kept[] = [new Kept(), new Sub()];
for (let i = 0; i < 16 * RUN; i++) mixed[i & 1].set(i, i - 8);
console.log("override", mixed[0].cells.join(","), mixed[1].cells.join(","), mixed[0].h, mixed[1].h);

// 8. A parameter the body writes, and one a closure captures.
class Written {
  cells: number[] = new Array<number>(16).fill(0);
  set(square: number, piece: number): number {
    if (square < 0) square = -square;
    const read = () => piece;
    this.cells[square] = read();
    return this.cells[square] ?? -1;
  }
}
const w = new Written();
let ws = 0;
for (let i = 0; i < 24 * RUN; i++) ws += w.set(i - 8, i);
console.log("written", ws, w.cells.join(","));

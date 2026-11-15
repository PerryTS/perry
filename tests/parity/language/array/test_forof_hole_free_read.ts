// The element a for-of walk reads from an Array the whole-program facts call
// hole-free is a proven Number: check mode (`PERRY_VERIFY_FACTS=1`) tests the
// slot where the walk reads it (no hole, a Number). Every case prints what
// node prints. The negative controls are Arrays that legitimately hold
// holes, shrink or grow under the walk, or are read past their end: their
// reads make no such claim and must not trip the witness.
const RUN = Math.random() < 2 ? 1 : 0;
const many = process.argv.length > 100;

function use(m: number): number {
  return ((m & 127) * 3 + ((m >> 7) & 127)) | 0;
}
function show(v: number | undefined): string {
  return v === undefined ? "u" : Object.is(v, -0) ? "-0" : String(v);
}
function gen(n: number, seed: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(((i * 37 + seed) & 127) | (((i * 11 + seed * 3) & 127) << 7));
  return out;
}
function total(xs: readonly number[]): string {
  let s = 0;
  let holes = 0;
  for (const x of xs) {
    if (x === undefined) holes++;
    else s = (s + use(x)) | 0;
  }
  return s + "/" + holes + "/" + xs.length;
}
function list(xs: readonly (number | undefined)[]): string {
  let r = "";
  for (const x of xs) r += show(x) + ";";
  return r;
}

// (a) Hole-free lists: grown by push only, walked in a function, in a
// method over a field, in a closure, and by the module body.
console.log("a1", total(gen(40 * RUN, 5)));
class Bag {
  xs: number[] = [];
  add(v: number): void {
    this.xs.push(v);
  }
  sum(): number {
    let s = 0;
    for (const x of this.xs) s = (s + use(x)) | 0;
    return s;
  }
}
const bag = new Bag();
for (let i = 0; i < 20 * RUN; i++) bag.add(i * 131 + 7);
console.log("a2", bag.sum());
const fs: (() => number)[] = [];
for (const x of gen(6 * RUN, 9)) fs.push(() => use(x) + 1);
let a3 = 0;
for (const f of fs) a3 += f();
console.log("a3", a3);
let a4 = 0;
for (const x of gen(30 * RUN, 2)) a4 = (a4 + (x === undefined ? 100000 : use(x))) | 0;
console.log("a4", a4);

// (b) Doubles: NaN and -0 are Numbers, and the walk keeps them.
function halves(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 0.5 - 1);
  out.push(0 / (RUN - 1));
  out.push(-0 * RUN);
  out.push(2147483648 * RUN);
  out.push(-2147483649 * RUN);
  return out;
}
console.log("b1", list(halves(4 * RUN)));
let b2 = 0;
for (const x of halves(4 * RUN)) b2 += x === undefined ? 1 : x !== x ? 10 : 100;
console.log("b2", b2);

// (c) Legitimately holey Arrays: a sparse birth, an elision, a store past
// the end, a `length` store, a `delete`.
function sparse(n: number): number[] {
  const xs: number[] = new Array(n);
  for (let i = 0; i < n; i++) xs.push(i * 3 + RUN);
  return xs;
}
console.log("c1", total(sparse(4 * RUN)), list(sparse(3 * RUN)));
const elided = [1 * RUN, , 3, , 5] as number[];
console.log("c2", total(elided), list(elided));
function past(n: number): number[] {
  const xs = gen(n, 3);
  xs[n + 2 * RUN] = 7;
  return xs;
}
console.log("c3", total(past(5 * RUN)), list(past(2 * RUN)));
function stretched(n: number): number[] {
  const xs = gen(n, 4);
  xs.length = n + 3 * RUN;
  return xs;
}
console.log("c4", total(stretched(5 * RUN)), list(stretched(2 * RUN)));
function punched(n: number): number[] {
  const xs = gen(n, 6);
  delete xs[1 * RUN];
  return xs;
}
console.log("c5", total(punched(5 * RUN)), list(punched(3 * RUN)));
let c6 = "";
for (const x of sparse(3 * RUN)) c6 += show(x) + ";";
console.log("c6", c6);

// (d) An Array that shrinks, grows or gets a hole while it is walked.
function shrink(n: number, cut: number): string {
  const xs = gen(n, 8);
  let r = "";
  for (const x of xs) {
    r += show(x) + ";";
    if (x === xs[cut]) {
      xs.pop();
      xs.pop();
    }
  }
  return r + xs.length;
}
console.log("d1", shrink(8 * RUN, 2), shrink(8 * RUN, 100));
function grow(n: number): string {
  const xs = gen(n, 1);
  let r = "";
  for (const x of xs) {
    r += show(x) + ";";
    if (xs.length < n + 3) xs.push(x + 1);
  }
  return r + xs.length;
}
console.log("d2", grow(4 * RUN));
function pierce(n: number): string {
  const xs = gen(n, 12);
  let r = "";
  let k = 0;
  for (const x of xs) {
    r += show(x) + ";";
    if (k === 1) xs[n + 1] = 9;
    k++;
  }
  return r + xs.length;
}
console.log("d3", pierce(4 * RUN));
function shifted(n: number): string {
  const xs = gen(n, 13);
  xs.shift();
  xs.unshift(77, 78);
  xs.splice(1 * RUN, 2);
  return total(xs) + "|" + list(xs);
}
console.log("d4", shifted(6 * RUN));

// (e) Reads by index: under the length, past the end, with a default.
function byIndex(xs: readonly number[], extra: number): string {
  let r = "";
  for (let i = 0; i < xs.length + extra; i++) r += show(xs[i]) + ";";
  let s = 0;
  for (let i = 0; i < xs.length + extra; i++) s += xs[i] ?? 1000;
  return r + s;
}
console.log("e1", byIndex(gen(4 * RUN, 2), 0), byIndex(gen(4 * RUN, 2), 2 * RUN));
console.log("e2", byIndex(sparse(2 * RUN), 1 * RUN));
const fixed: number[] = [4, 5, 6, 7];
let e3 = 0;
for (const x of fixed) e3 += x;
for (let i = 0; i < fixed.length; i++) e3 += fixed[i] * 2;
console.log("e3", e3, show(fixed[many ? 1 : 9]));

// (f) A sorted copy written in bounds, then walked; and its holey twin.
function order(moves: readonly number[]): number[] {
  const result = moves.slice();
  for (let i = 1; i < result.length; i++) {
    const move = result[i] ?? 0;
    let j = i - 1;
    while (j >= 0 && (result[j] ?? 0) > move) {
      result[j + 1] = result[j] ?? 0;
      j--;
    }
    result[j + 1] = move;
  }
  return result;
}
console.log("f1", total(order(gen(12 * RUN, 7))), total(order(sparse(3 * RUN))));

// (g) Elements that are not all Numbers, and an Array of Arrays.
const mixed: (number | string)[] = [1, "two", 3];
if (RUN) mixed.push("four", 5);
let g1 = "";
for (const x of mixed) g1 += typeof x === "number" ? x + 1 : x.length;
console.log("g1", g1);
const rows: number[][] = [];
for (let i = 0; i < 3 * RUN; i++) rows.push(gen(i + 1, i));
let g2 = 0;
for (const row of rows) for (const x of row) g2 = (g2 + use(x)) | 0;
console.log("g2", g2);

// (h) A field of small ints read in its floor, and walked.
class Board {
  cells: number[] = new Array(16).fill(0);
  set(i: number, v: number): void {
    this.cells[i & 15] = v | 0;
  }
  sum(): number {
    let s = 0;
    for (const c of this.cells) s += c;
    for (let i = 0; i < 16; i++) s += (this.cells[i] ?? 0) * 2;
    return s;
  }
}
const board = new Board();
for (let i = 0; i < 40 * RUN; i++) board.set(i * 7, i - 20);
console.log("h1", board.sum());

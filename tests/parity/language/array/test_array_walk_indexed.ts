// The indexed walk (CENSUS-residual-2 lever 6): `for (const d of xs)` where
// `xs` is a `const` that selects among module-const Number arrays. Every
// Array it may hold is immutable, hole-free and of a fixed length, so the
// loop reads the length once and each element as its slot. The first half
// is loops the proof covers; the second half is the negative controls, where
// one part of the proof is missing and the loop must still do what the array
// iterator does.
const out: string[] = [];

const KNIGHT = [-33, -31, -18, -14, 14, 18, 31, 33];
const KING = [-17, -16, -15, -1, 1, 15, 16, 17];
const BISHOP = [-17, -15, 15, 17];
const ROOK = [-16, -1, 1, 16];
const LONG = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7, 9, 3, 2, 3, 8, 4];
const HALVES = [0.5, 1.5, -2.25, 1e21, 4294967296.5];
const WHOLE = [1, 2, 3];
const SPECIAL = [NaN, -0, Infinity, -Infinity, 0];

function onBoard(s: number): boolean {
  return (s & 0x88) === 0;
}
const board: number[] = [];
for (let i = 0; i < 128; i++) board.push((i * 7) % 5 === 0 ? ((i % 3) - 1) * 2 : 0);

// ---- covered: a select of immutable int arrays (move generation) ----
function encode(from: number, to: number): number {
  return from | (to << 7);
}
function generate(type: number, from: number, color: number): number[] {
  const moves: number[] = [];
  const directions = type === 2 ? KNIGHT : type === 3 ? BISHOP : type === 4 ? ROOK : KING;
  for (const delta of directions) {
    let to = from + delta;
    while (onBoard(to)) {
      const target = board[to];
      if (target * color > 0) break;
      moves.push(encode(from, to));
      if (target !== 0 || type === 2 || type === 6) break;
      to += delta;
    }
  }
  return moves;
}
let gen = 0;
let count = 0;
for (let type = 2; type <= 6; type++) {
  for (let from = 0; from < 128; from++) {
    if (!onBoard(from)) continue;
    const ms = generate(type, from, type % 2 === 0 ? 1 : -1);
    count += ms.length;
    for (const m of ms) gen = (gen * 31 + m) % 1000003;
  }
}
out.push("generate " + count + " " + gen);

// `switch` selects the Array.
function pick(kind: number): number[] {
  switch (kind) {
    case 0:
      return KNIGHT;
    case 1:
      return BISHOP;
    default:
      return ROOK;
  }
}
function viaSwitch(kind: number): string {
  let xs = ROOK;
  switch (kind) {
    case 0:
      xs = KNIGHT;
      break;
    case 1:
      xs = BISHOP;
      break;
  }
  let s = "";
  for (const d of xs) s += d + ",";
  let t = 0;
  for (const d of pick(kind)) t += d * d;
  return s + t;
}
out.push("switch " + viaSwitch(0) + "|" + viaSwitch(1) + "|" + viaSwitch(2));

// `break`, `continue`, `return` and a labeled jump in the body.
function jumps(type: number, stop: number): string {
  const xs = type === 0 ? KNIGHT : type === 1 ? LONG : ROOK;
  let s = "";
  let n = 0;
  for (const d of xs) {
    n++;
    if (d < 0) continue;
    if (d === stop) break;
    if (d === 16) return s + "ret@" + n;
    s += d + ";";
  }
  return s + "end@" + n;
}
out.push("jumps " + jumps(0, 18) + " " + jumps(0, 99) + " " + jumps(1, 9) + " " + jumps(2, 7));
function labeled(type: number): number {
  const xs = type === 0 ? BISHOP : ROOK;
  const ys = type === 0 ? ROOK : KING;
  let n = 0;
  outer: for (const a of xs) {
    for (const b of ys) {
      if (a + b === -18) continue outer;
      if (a * b === -240) break outer;
      n += a * b;
    }
    n += 1000;
  }
  return n;
}
out.push("labeled " + labeled(0) + " " + labeled(1));

// More elements than an unrolled walk has: one Array, and a select.
let long = 0;
for (const v of LONG) long = (long * 7 + v) % 100003;
function longOr(k: number): number {
  const xs = k === 0 ? LONG : KING;
  let s = 0;
  let i = 0;
  for (const v of xs) s += v * ++i;
  return s;
}
out.push("long " + long + " " + longOr(0) + " " + longOr(1));

// Non-integer elements: the element is the Number, not an int32.
function halves(k: number): string {
  const xs = k === 0 ? HALVES : WHOLE;
  let s = "";
  for (const v of xs) s += v / 2 + ";";
  return s;
}
out.push("halves " + halves(0) + "|" + halves(1));
function special(k: number): string {
  const xs = k === 0 ? SPECIAL : WHOLE;
  let s = "";
  for (const v of xs) s += (Object.is(v, -0) ? "-0" : String(v)) + ";";
  return s;
}
out.push("special " + special(0) + "|" + special(1));

// A user-written indexed loop over the same kind of local.
function indexed(type: number): number {
  const xs = type === 2 ? KNIGHT : ROOK;
  let s = 0;
  for (let i = 0; i < xs.length; i++) s = s * 3 + xs[i];
  for (let i = 1; i < xs.length; i++) s += xs[i] - xs[i - 1];
  return s;
}
out.push("indexed " + indexed(2) + " " + indexed(4));

// The body allocates and collects while the walk holds its length.
function churn(type: number): number {
  const xs = type === 0 ? KNIGHT : KING;
  let total = 0;
  for (const d of xs) {
    const junk: number[][] = [];
    for (let i = 0; i < 400; i++) junk.push([i, d, i + d]);
    total += junk[399][2] + junk.length;
  }
  return total;
}
let churned = 0;
for (let r = 0; r < 60; r++) churned += churn(r % 2);
out.push("churn " + churned);

// Closures capture one binding per iteration; a `let` binding is the
// iteration's own.
function captures(type: number): string {
  const xs = type === 0 ? BISHOP : ROOK;
  const fs: Array<() => number> = [];
  for (const d of xs) fs.push(() => d * 2);
  let s = fs.map((f) => f()).join(",");
  for (let d of xs) {
    d += 100;
    s += ";" + d;
  }
  return s;
}
out.push("captures " + captures(0) + "|" + captures(1));

// `try`/`finally` around the jumps, and a throw out of the walk.
function guarded(type: number, bad: number): string {
  const xs = type === 0 ? ROOK : BISHOP;
  let s = "";
  try {
    for (const d of xs) {
      try {
        if (d === bad) throw new Error("bad " + d);
        if (d === -1) continue;
        s += d;
      } finally {
        s += "f";
      }
    }
  } catch (e) {
    s += "[" + (e as Error).message + "]";
  }
  return s;
}
out.push("guarded " + guarded(0, 1) + " " + guarded(0, 99) + " " + guarded(1, 15));

// ---- negative controls ----

// An Array that is written somewhere, at any time, is not immutable.
const WRITTEN = [1, 2, 3, 4];
const CLEAN = [5, 6, 7, 8];
function sumSel(k: number): number {
  const xs = k === 0 ? WRITTEN : CLEAN;
  let s = 0;
  for (const v of xs) s += v;
  return s;
}
const before = sumSel(0) + "," + sumSel(1);
WRITTEN[2] = 30;
out.push("written " + before + " " + sumSel(0) + "," + sumSel(1));

// Written through an alias, in a callee.
const ALIASED = [1, 2, 3];
const OTHER = [4, 5, 6];
function poke(a: number[], i: number, v: number): void {
  a[i] = v;
}
function sumAliased(k: number): number {
  const xs = k === 0 ? ALIASED : OTHER;
  let s = 0;
  for (const v of xs) s = s * 10 + v;
  return s;
}
poke(ALIASED, 1, 9);
out.push("aliased " + sumAliased(0) + " " + sumAliased(1));

// Holes: an elision, and a `new Array(n)`.
const HOLED = [1, , 3, , 5];
const FULL = [1, 2, 3, 4, 5];
const BARE: number[] = new Array(3);
function holes(k: number): string {
  const xs = k === 0 ? HOLED : k === 1 ? FULL : BARE;
  let s = "";
  for (const v of xs) s += (v === undefined ? "u" : v) + ",";
  return s;
}
out.push("holes " + holes(0) + "|" + holes(1) + "|" + holes(2));

// Elements that are not Numbers, in one arm of the select.
const MIXED: Array<number | string> = [1, "two", 3];
const NUMS: Array<number | string> = [4, 5, 6];
const NULLS: Array<number | null | undefined> = [1, null, undefined, 4];
function mixed(k: number): string {
  const xs = k === 0 ? MIXED : NUMS;
  let s = "";
  for (const v of xs) s += typeof v + ":" + v + ",";
  for (const v of NULLS) s += v + "/";
  return s;
}
out.push("mixed " + mixed(0) + "|" + mixed(1));

// The iterator protocol of one Array is replaced: it is no plain Array, and
// a select that may hold it runs the iterator the program installed.
const OWN = [1, 2, 3];
(OWN as any)[Symbol.iterator] = function* () {
  yield 70;
  yield 80;
};
const PLAIN = [10, 20, 30];
function protocol(k: number): string {
  const xs = k === 0 ? OWN : PLAIN;
  let s = "";
  for (const v of xs) s += v + ",";
  return s;
}
out.push("protocol " + protocol(0) + "|" + protocol(1));

// Mutation during iteration: the Array grows, shrinks and is stored into
// while the loop runs, so the loop sees the length and the elements of each
// step.
const GROWS = [1, 2, 3];
const STAYS = [7, 8, 9];
function during(k: number): string {
  const xs = k === 0 ? GROWS : STAYS;
  let s = "";
  for (const v of xs) {
    s += v + ",";
    if (k === 0 && xs.length < 6) xs.push(v * 10);
  }
  return s;
}
out.push("grows " + during(0) + "|" + during(1) + "|" + GROWS.length);
const SHRINKS = [1, 2, 3, 4, 5, 6];
function shrinking(): string {
  let s = "";
  for (const v of SHRINKS) {
    s += v + ",";
    if (v === 2) SHRINKS.length = 4;
    if (v === 3) SHRINKS[3] = 40;
  }
  return s;
}
out.push("shrinks " + shrinking() + " " + SHRINKS.length);
const POPS = [1, 2, 3, 4];
const FIXED = [5, 6, 7, 8];
function popping(k: number): string {
  const xs = k === 0 ? POPS : FIXED;
  let s = "";
  for (let i = 0; i < xs.length; i++) {
    s += xs[i] + ",";
    if (k === 0 && i === 1) POPS.pop();
  }
  return s;
}
out.push("pops " + popping(0) + "|" + popping(1));

// The counter of a user loop is written by the body, or by a closure the
// body calls: the index is not below the length in every iteration.
function counterWritten(type: number): string {
  const xs = type === 0 ? BISHOP : ROOK;
  let s = "";
  let jumped = false;
  for (let i = 0; i < xs.length; i++) {
    const v = xs[i];
    s += (v === undefined ? "u" : v) + ",";
    if ((v === 15 || v === 1) && !jumped) {
      jumped = true;
      i = -3;
    }
  }
  return s;
}
out.push("counter " + counterWritten(0) + "|" + counterWritten(1));
function counterCaptured(type: number): string {
  const xs = type === 0 ? BISHOP : ROOK;
  let s = "";
  let i = 0;
  const skip = () => {
    i += 2;
  };
  for (i = 0; i < xs.length; i++) {
    if (xs[i] === -15 || xs[i] === 1) skip();
    s += (xs[i] === undefined ? "u" : xs[i]) + ",";
  }
  return s;
}
out.push("captured " + counterCaptured(0) + "|" + counterCaptured(1));
function negativeStart(type: number): string {
  const xs = type === 0 ? BISHOP : ROOK;
  let s = "";
  for (let i = -2; i < xs.length; i++) s += (xs[i] === undefined ? "u" : xs[i]) + ",";
  for (let i = 0; i <= xs.length; i++) s += (xs[i] === undefined ? "u" : xs[i]) + ";";
  return s;
}
out.push("bounds " + negativeStart(0) + "|" + negativeStart(1));

// A binding that is no rooted `const`: a `let` that is reassigned, and a
// parameter that also receives a written Array.
function reassigned(k: number): string {
  let xs = k === 0 ? KNIGHT : ROOK;
  let s = "";
  for (const v of xs) {
    s += v + ",";
    if (v === -1) xs = BISHOP;
  }
  for (const v of xs) s += v + ";";
  return s;
}
out.push("let " + reassigned(0) + "|" + reassigned(1));
function total(xs: number[]): number {
  let s = 0;
  for (const v of xs) s += v;
  return s;
}
out.push("param " + total(KNIGHT) + "," + total(WRITTEN) + "," + total(HALVES) + "," + total(GROWS));

// An empty Array in the select.
const EMPTY: number[] = [];
function maybeEmpty(k: number): string {
  const xs = k === 0 ? EMPTY : WHOLE;
  let s = "[";
  for (const v of xs) s += v;
  return s + "]";
}
out.push("empty " + maybeEmpty(0) + maybeEmpty(1));

console.log(out.join("\n"));

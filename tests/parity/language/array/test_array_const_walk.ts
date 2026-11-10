// Fact IM (immutable constant Arrays): `for (const x of A)` over an Array
// nothing writes after its birth, born from Number literals, is its
// iterations in order with `x` a constant. Each loop below must see exactly
// what the array iterator yields. The second half is the negative controls:
// an Array that is written, in any way and at any time, is not immutable.
const out: string[] = [];

// ---- immutable: unrolled ----
const KNIGHT = [-33, -31, -18, -14, 14, 18, 31, 33];
const KING = [-17, -16, -15, -1, 1, 15, 16, 17];
const BISHOP = [-17, -15, 15, 17];
const ROOK = [-16, -1, 1, 16];
const ODD = [0.5, -0, NaN, 1e21, -2.25, 4294967296, -2147483649];
const EMPTY: number[] = [];
const ONE = [7];
const NINE = [1, 2, 3, 4, 5, 6, 7, 8, 9];

const board: number[] = [];
for (let i = 0; i < 128; i++) board.push((i * 7) % 5 === 0 ? ((i % 3) - 1) * 2 : 0);

function onBoard(s: number): boolean {
  return (s & 0x88) === 0;
}

// Early return, a nested while, `break` out of the inner loop only, and a
// flag folded per element (`diagonal`).
function attacked(square: number, color: number): boolean {
  for (const delta of KNIGHT) {
    const source = square + delta;
    if (onBoard(source) && board[source] === color * 2) return true;
  }
  for (const delta of KING) {
    let source = square + delta;
    let distance = 1;
    while (onBoard(source)) {
      const piece = board[source];
      if (piece !== 0) {
        const diagonal = delta === -17 || delta === -15 || delta === 15 || delta === 17;
        if (piece * color > 0 && (diagonal || distance === 1)) return true;
        break;
      }
      source += delta;
      distance++;
    }
  }
  return false;
}
let hits = 0;
for (let s = 0; s < 128; s++) {
  if (onBoard(s)) {
    if (attacked(s, 1)) hits++;
    if (attacked(s, -1)) hits += 100;
  }
}
out.push("attacked " + hits);

// `continue`, `break`, and what follows the loop.
function firstBig(limit: number): string {
  let seen = "";
  let last = 0;
  for (const d of KNIGHT) {
    if (d < 0) continue;
    if (d > limit) break;
    seen += d + ",";
    last = d;
  }
  return seen + "|" + last;
}
out.push("jumps " + firstBig(18) + " " + firstBig(100) + " " + firstBig(0));

// The body reads the Array's length and indexes it.
let lens = 0;
for (const d of ROOK) lens += ROOK.length * d + ROOK[0] + KNIGHT[KNIGHT.length - 1];
out.push("length " + lens);

// A `let` binding the body writes; each iteration has its own.
let bumped = "";
for (let d of BISHOP) {
  d += 100;
  bumped += d + ";";
}
out.push("let " + bumped);

// Closures capture one binding per iteration.
const fs: Array<() => number> = [];
for (const d of ROOK) fs.push(() => d * 2);
out.push("closures " + fs.map((f) => f()).join(","));

// Non-integer, negative zero, NaN, beyond int32.
let odd = "";
for (const v of ODD) odd += (Object.is(v, -0) ? "-0" : String(v)) + ";";
out.push("odd " + odd);

// Empty, one element, more than the unroll bound.
let counts = "";
for (const v of EMPTY) counts += "never" + v;
for (const v of ONE) counts += "one" + v;
let nine = 0;
for (const v of NINE) nine = nine * 10 + v;
out.push("sizes " + counts + " " + nine);

// Nested walks over immutable Arrays.
let pairs = 0;
for (const a of BISHOP) {
  for (const b of ROOK) {
    if (a + b > 0) pairs += a * b;
    else if (a + b === 0) continue;
    else pairs--;
  }
}
out.push("nested " + pairs);

// One loop over one of several immutable Arrays.
function walkEither(type: number): string {
  const directions = type === 2 ? KNIGHT : type === 3 ? BISHOP : type === 4 ? ROOK : KING;
  let s = "";
  for (const d of directions) {
    if (d === 1) continue;
    s += d + ";";
  }
  return s;
}
out.push("either " + walkEither(2) + "|" + walkEither(3) + "|" + walkEither(4) + "|" + walkEither(5));

// A parameter that only ever holds immutable Arrays.
function sumOf(xs: number[]): number {
  let s = 0;
  for (const d of xs) s += d;
  return s;
}
out.push("param " + sumOf(ROOK) + "," + sumOf(BISHOP) + "," + sumOf(NINE));

// `switch` in the body: its `break` leaves the switch, `continue` the copy.
let sw = "";
for (const d of ROOK) {
  switch (d) {
    case -1:
      sw += "w";
      break;
    case 1:
      continue;
    default:
      sw += "d" + d;
  }
  sw += ".";
}
out.push("switch " + sw);

// `try`/`finally` around `break`, `continue` and `return`.
function guarded(stop: number): string {
  let log = "";
  for (const d of BISHOP) {
    try {
      if (d === -15) continue;
      if (d === stop) break;
      if (d === 17) return log + "ret";
      log += d;
    } finally {
      log += "f";
    }
  }
  return log + "end";
}
out.push("try " + guarded(15) + " " + guarded(99));

// A throw from the body.
function thrower(): string {
  try {
    for (const d of ROOK) {
      if (d === 1) throw new Error("at " + d);
    }
  } catch (e) {
    return (e as Error).message;
  }
  return "none";
}
out.push("throw " + thrower());

// A labeled loop around it.
let lab = 0;
outer: for (let r = 0; r < 3; r++) {
  for (const d of ROOK) {
    if (d === 16 && r === 1) continue outer;
    if (d === 1 && r === 2) break outer;
    lab += d + r;
  }
  lab += 1000;
}
out.push("labeled " + lab);

// A generator and an async function.
function* gen(): Generator<number> {
  for (const d of BISHOP) {
    if (d === 15) continue;
    yield d * 3;
  }
}
out.push("gen " + [...gen()].join(","));

async function later(): Promise<string> {
  let s = "";
  for (const d of ROOK) {
    s += await Promise.resolve(d);
  }
  return s;
}

// A literal in a function: each call births its own.
function local(n: number): number {
  const steps = [3, 5, 9];
  let s = n;
  for (const d of steps) s = s * 2 + d;
  return s;
}
out.push("local " + local(1) + "," + local(2));

// Elements that read a module `const` holding one Number, and ones that
// read a binding that changes.
const NORTH = -16;
const EAST = 1 << 0;
const ZERO = 0;
const DIRS = [NORTH, -NORTH, EAST, -EAST, -ZERO];
let dirs = "";
for (const d of DIRS) dirs += (Object.is(d, -0) ? "-0" : d) + ";";
let step = 2;
function stepped(): number[] {
  return [step, -step, 1];
}
let st = "";
for (const d of stepped()) st += d + ";";
step = 5;
const laterSteps = stepped();
for (const d of laterSteps) st += d + ";";
out.push("consts " + dirs + " " + st + DIRS[1] + NINE[8] + KNIGHT[0]);

// A hoisted `var` read before its initializer runs holds `undefined`, and a
// `const` read in its dead zone throws: neither walk may run on the
// literal's values before the binding holds the Array.
function earlyVar(): string {
  let s = "";
  for (const x of HOISTED) s += x;
  return s;
}
function earlyConst(): string {
  let s = "";
  for (const x of LATER) s += x;
  return s;
}
let hoisted = "";
try {
  hoisted = earlyVar();
} catch (e) {
  hoisted = "TypeError " + (e instanceof TypeError);
}
let dead = "";
try {
  dead = earlyConst();
} catch (e) {
  dead = "ReferenceError " + (e instanceof ReferenceError);
}
var HOISTED = [1, 2, 3];
const LATER = [4, 5, 6];
out.push("early " + hoisted + " " + dead + " " + earlyVar() + earlyConst());

// ---- negative controls: written, so not immutable ----
// Written later through an alias: the first walk sees the birth values.
const W = [1, 2, 3, 4];
let w1 = 0;
for (const v of W) w1 = w1 * 10 + v;
const alias = W;
alias[1] = 7;
let w2 = 0;
for (const v of W) w2 = w2 * 10 + v;
out.push("alias " + w1 + "," + w2);

// Written in a callee.
const C = [1, 2, 3, 4];
function poke(a: number[], i: number, v: number): void {
  a[i] = v;
}
poke(C, 2, 9);
let c1 = 0;
for (const v of C) c1 = c1 * 10 + v;
out.push("callee " + c1);

// Written in a closure that runs later.
const L = [1, 2, 3];
const writeL = () => {
  L[0] = 5;
};
let l1 = 0;
for (const v of L) l1 = l1 * 10 + v;
writeL();
for (const v of L) l1 = l1 * 10 + v;
out.push("closure " + l1);

// Pushed to; popped; `length` stored.
const P = [1, 2, 3];
P.push(4);
let p1 = 0;
for (const v of P) p1 = p1 * 10 + v;
const Q = [1, 2, 3];
Q.pop();
for (const v of Q) p1 = p1 * 10 + v;
const R = [1, 2, 3];
R.length = 1;
for (const v of R) p1 = p1 * 10 + v;
out.push("resized " + p1);

// In-place methods that keep the length.
const S1 = [3, 1, 2];
S1.sort();
const S2 = [3, 1, 2];
S2.reverse();
const S3 = [3, 1, 2];
S3.fill(8, 1);
const S4 = [3, 1, 2];
S4.copyWithin(0, 1);
const S5 = [3, 1, 2];
Array.prototype.sort.call(S5);
const S6 = [3, 1, 2];
S6.splice(1, 1, 9);
let inplace = "";
for (const a of [S1, S2, S3, S4, S5, S6]) {
  for (const v of a) inplace += v;
  inplace += ";";
}
let direct = "";
for (const v of S1) direct += v;
for (const v of S2) direct += v;
for (const v of S3) direct += v;
for (const v of S4) direct += v;
for (const v of S5) direct += v;
for (const v of S6) direct += v;
out.push("inplace " + inplace + " " + direct);

// Updates, compound stores, destructuring stores.
const U = [1, 2, 3];
U[0]++;
U[1] += 5;
[U[2], U[0]] = [U[0], U[2]];
let u1 = "";
for (const v of U) u1 += v + ",";
out.push("updates " + u1);

// `Object.assign` and `delete`. (`Object.defineProperty` of an index and
// `Reflect.set` turn the walk's proof off for the whole program: they are in
// test_array_const_walk_own_iterator.ts.)
const O1 = [1, 2, 3];
Object.assign(O1, { 1: 20 });
const O4 = [1, 2, 3];
delete O4[1];
let objs = "";
for (const v of O1) objs += v + ",";
for (const v of O4) objs += v + ",";
out.push("object " + objs);

// Frozen (never written, and a store into it does nothing).
const F = [1, 2, 3];
Object.freeze(F);
try {
  (F as number[])[1] = 9;
} catch (e) {
  out.push("frozen store threw");
}
let f1 = "";
for (const v of F) f1 += v + ",";
out.push("frozen " + f1 + Object.isFrozen(F));

// Escaped: to JSON.stringify, to a function value nobody knows.
const J = [1, 2, 3];
const text = JSON.stringify(J);
let j1 = "";
for (const v of J) j1 += v;
const X = [1, 2, 3];
const unknown: (a: number[]) => void = (globalThis as any).perryNoSuchHook ?? ((a: number[]) => {
  a[2] = 30;
});
unknown(X);
for (const v of X) j1 += "," + v;
out.push("escaped " + text + " " + j1);

// Written by the loop that walks it.
const M = [1, 2, 3, 4];
let m1 = "";
for (const v of M) {
  if (v === 1) M[2] = 30;
  m1 += v + ",";
}
out.push("walked " + m1);

// A holey literal, non-Number elements, computed elements.
const H = [1, , 3];
let h1 = "";
for (const v of H) h1 += v + ",";
const T = ["a", "b"];
for (const v of T) h1 += v + ",";
const MIX = [1, "x", null, undefined, true];
for (const v of MIX) h1 += v + ",";
const OBJ = [{ k: 1 }, { k: 2 }];
for (const v of OBJ) h1 += v.k + ",";
const seed = out.length;
const COMP = [seed, seed + 1, 2 * 3];
for (const v of COMP) h1 += v + ",";
out.push("kinds " + h1);

// A named property on the Array.
const N = [1, 2, 3];
(N as any).tag = "t";
let n1 = "";
for (const v of N) n1 += v;
out.push("named " + n1 + (N as any).tag);

later().then((s) => {
  out.push("async " + s);
  console.log(out.join("\n"));
});

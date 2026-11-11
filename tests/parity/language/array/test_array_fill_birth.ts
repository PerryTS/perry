// The fill birth: `new Array(k).fill(v)` of an integer literal `k` is born
// once, in its final representation (one allocation, each element stored
// once; inline for a small birth of a plain double or an I32 word, else one
// runtime entry). Covered: every length class (0, 1, below and at the
// runtime's floor of four slots, the inline bound of sixteen, past it, large),
// every value class (0, -0, NaN, a fraction, Infinity, an int past int32,
// undefined, null, a boolean, a string, an object, an Array, a bigint, a
// value whose annotation lies), what the born Array is afterwards (length,
// holes, reads past the length, a later push, a store past the capacity, a
// length cut, delete, every builtin reader), at-rest births with and without
// a reserved capacity, the order of evaluation, and births in a loop across
// collections. Negative controls that keep the two-step birth and must match:
// a length that is no literal (and a RangeError for a negative or a
// fractional one, thrown before the fill value is evaluated), a non-number
// constructor argument, `fill` with a start and an end, a fill on a named
// Array, the two-argument constructor, a subclass.

function show(tag: string, a: unknown[]): void {
  const parts: string[] = [];
  for (let i = 0; i < a.length; i++) {
    const v = a[i];
    parts.push(
      (i in a ? "" : "hole:") +
        (typeof v === "number" && Object.is(v, -0)
          ? "-0"
          : typeof v === "bigint"
            ? v.toString() + "n"
            : typeof v === "object" && v !== null
              ? JSON.stringify(v)
              : String(v)),
    );
  }
  console.log(tag, a.length, "[" + parts.join(",") + "]");
}

function churn(n: number): number {
  // Allocation pressure, so collections run between births and reads.
  let keep: { v: number }[] = [];
  let s = 0;
  for (let i = 0; i < n; i++) {
    keep.push({ v: i });
    if (keep.length > 64) keep = [];
    s = (s + keep.length) | 0;
  }
  return s;
}

// --- lengths ---
show("len0", new Array<number>(0).fill(7));
show("len1", new Array<number>(1).fill(7));
show("len2", new Array<number>(2).fill(7));
show("len3", new Array<number>(3).fill(7));
show("len4", new Array<number>(4).fill(7));
show("len5", new Array<number>(5).fill(7));
show("len8", new Array<number>(8).fill(0));
show("len16", new Array<number>(16).fill(3));
show("len17", new Array<number>(17).fill(3));
const big = new Array<number>(5000).fill(2);
let bigSum = 0;
for (let i = 0; i < big.length; i++) bigSum += big[i];
console.log("len5000", big.length, bigSum, big[0], big[4999], big[5000]);
const huge = new Array<number>(300000).fill(1.5);
let hugeSum = 0;
for (let i = 0; i < huge.length; i++) hugeSum += huge[i];
console.log("len300000", huge.length, hugeSum, huge[299999], huge[300000]);

// --- values ---
show("zero", new Array<number>(4).fill(0));
const negZero = new Array<number>(4).fill(-0);
show("negzero", negZero);
console.log("negzero sign", 1 / negZero[0], Object.is(negZero[3], -0), Object.is(negZero[3], 0));
const nans = new Array<number>(3).fill(NaN);
show("nan", nans);
console.log("nan reads", nans[0] !== nans[0], nans.includes(NaN), nans.indexOf(NaN));
show("fraction", new Array<number>(3).fill(0.1));
show("infinity", new Array<number>(3).fill(-Infinity));
show("past int32", new Array<number>(3).fill(4294967296));
show("min int32", new Array<number>(3).fill(-2147483648));
show("undefined", new Array<undefined>(3).fill(undefined));
show("null", new Array<null>(3).fill(null));
show("true", new Array<boolean>(3).fill(true));
show("string", new Array<string>(3).fill("ab"));
show("empty string", new Array<string>(3).fill(""));
show("bigint", new Array<bigint>(3).fill(12345678901234567890n));
const obj = { k: 1 };
const objs = new Array<{ k: number }>(5).fill(obj);
objs[0].k = 9;
console.log("object", objs.length, objs[4].k, objs[0] === objs[4], objs[2] === obj);
const inner = new Array<number[]>(3).fill(new Array<number>(2).fill(7));
inner[0][1] = 8;
console.log("nested", JSON.stringify(inner), inner[0] === inner[2]);
const lie: number = "str" as unknown as number;
show("lying annotation", new Array<number>(4).fill(lie));
const boxedLie: number = { x: 1 } as unknown as number;
const lies = new Array<number>(4).fill(boxedLie);
churn(40000);
console.log("lying object", lies.length, JSON.stringify(lies[3]), lies[0] === lies[3]);
function pass<T>(v: T): T {
  return v;
}
show("call value", new Array<number>(4).fill(pass(6.25)));
show("call string", new Array<unknown>(4).fill(pass("s")));
let counter = 0;
function next(): number {
  counter += 1;
  return counter;
}
show("effect value", new Array<number>(3).fill(next()));
show("effect value again", new Array<number>(3).fill(next()));
console.log("evaluated", counter);
function boom(): number {
  throw new Error("boom");
}
try {
  show("unreached", new Array<number>(3).fill(boom()));
} catch (e) {
  console.log("value threw", (e as Error).message);
}
for (let i = 0; i < 3; i++) {
  show("loop value " + i, new Array<number>(4).fill(i * 2 + 0.5));
}
for (let i = 0; i < 3; i++) {
  show("loop int " + i, new Array<number>(4).fill(i));
}

// --- the born Array afterwards ---
const a = new Array<number>(8).fill(1);
console.log("reads", a[0], a[7], a[8], a[-1], a[100], a.length, 0 in a, 7 in a, 8 in a);
a.push(2);
a.push(3);
console.log("push", a.length, a[8], a[9], a[10], JSON.stringify(a));
a[20] = 5;
console.log("grow", a.length, a[19], a[20], 19 in a, 20 in a, a.indexOf(5));
a.length = 3;
console.log("cut", a.length, JSON.stringify(a), a[3]);
delete a[1];
console.log("delete", a.length, 1 in a, a[1], JSON.stringify(a));
const b = new Array<number>(6).fill(4);
b[2] = 10;
b[5] = -1;
console.log(
  "readers",
  b.join("|"),
  b.indexOf(10),
  b.includes(-1),
  b.reduce((x, y) => x + y, 0),
  b.map((x) => x * 2).join(","),
  b.filter((x) => x > 4).length,
  b.slice(1, 3).join(","),
  [...b].length,
  Object.keys(b).join(""),
  b.concat([1]).length,
  Math.max(...b),
);
b.sort((x, y) => x - y);
b.reverse();
console.log("sorted", b.join(","), Array.isArray(b), b instanceof Array, b.constructor === Array);
let forOf = 0;
for (const v of new Array<number>(5).fill(3)) forOf += v;
const [d0, d1, ...dRest] = new Array<number>(4).fill(9);
console.log("for-of", forOf, "destructure", d0, d1, dRest.length);
const s = new Array<string>(3).fill("x");
s[1] = "y";
s.push("z");
console.log("strings", s.join(""), s.length);
const mixed = new Array<number | string>(4).fill(0);
mixed[1] = "one";
mixed[2] = 2.5;
console.log("mixed", JSON.stringify(mixed));
const frozen = Object.freeze(new Array<number>(3).fill(1));
console.log("frozen", Object.isFrozen(frozen), frozen.length, frozen[2]);
const holes = new Array<undefined>(3).fill(undefined);
console.log("undefined is no hole", 0 in holes, Object.keys(holes).length, holes.length);
const copy = new Array<number>(4).fill(2);
copy.fill(5, 1, 3);
copy.fill(6, -1);
console.log("refill", JSON.stringify(copy));

// --- at-rest births ---
const cells = new Array<number>(8).fill(0);
function put(i: number, v: number): void {
  cells[i & 7] = v & 255;
}
function get(i: number): number {
  return cells[i & 7] | 0;
}
let restSum = 0;
for (let i = 0; i < 100; i++) {
  put(i, i * 3);
  restSum = (restSum + get(i + 1)) | 0;
}
console.log("at rest", restSum, cells.length, get(0), get(7));
const ones = new Array<number>(8).fill(7);
function bump(i: number): void {
  ones[i & 7] = (ones[i & 7] + 1) | 0;
}
for (let i = 0; i < 20; i++) bump(i);
let onesSum = 0;
for (let i = 0; i < 8; i++) onesSum = (onesSum + (ones[i & 7] | 0)) | 0;
console.log("at rest nonzero", onesSum, ones.length);
const roomy = new Array<number>(8).fill(1);
function putRoomy(i: number, v: number): void {
  roomy[i & 15] = v & 0x7ffff000;
}
function getRoomy(i: number): number {
  return roomy[i & 15] ?? -1;
}
putRoomy(12, 4096 * 7);
console.log("reserved", roomy.length, getRoomy(3), getRoomy(8), getRoomy(12), getRoomy(15));
class Board {
  private cells: number[] = new Array<number>(128).fill(0);
  private small: number[] = new Array<number>(8).fill(0);
  set(i: number, v: number): void {
    this.cells[i & 127] = v & 15;
    this.small[i & 7] = v & 3;
  }
  at(i: number): number {
    return (this.cells[i & 127] | 0) + (this.small[i & 7] | 0);
  }
}
const board = new Board();
let boardSum = 0;
for (let i = 0; i < 300; i++) {
  board.set(i, i);
  boardSum = (boardSum + board.at(i)) | 0;
}
console.log("board", boardSum, board.at(5), board.at(127));

// --- births in a loop, across collections ---
function files(seed: number): number {
  const white = new Array<number>(8).fill(0);
  const black = new Array<number>(8).fill(0);
  for (let i = 0; i < 8; i++) {
    white[i] += (seed + i) & 3;
    black[(i * 3) & 7] += 1;
  }
  let t = 0;
  for (let i = 0; i < 8; i++) t += white[i] * 2 + black[i];
  return t;
}
let filesSum = 0;
for (let i = 0; i < 20000; i++) filesSum = (filesSum + files(i)) | 0;
console.log("loop births", filesSum);
const ring: number[][] = [];
let ringSum = 0;
for (let i = 0; i < 30000; i++) {
  const r = new Array<number>(6).fill(i % 5);
  r[i % 6] = i;
  ring[i & 31] = r;
  if ((i & 1023) === 0) churn(3000);
  const old = ring[(i + 7) & 31];
  if (old !== undefined) ringSum = (ringSum + old[0] + old[5] + old.length) | 0;
}
console.log("ring", ringSum, ring.length, JSON.stringify(ring[5]));
const kept: string[][] = [];
for (let i = 0; i < 2000; i++) {
  kept.push(new Array<string>(3).fill("k" + (i % 7)));
  if ((i & 255) === 0) churn(2000);
}
console.log("string births", kept.length, kept[0].join(""), kept[1999].join(""), kept[1000][2]);

// --- negative controls: the two-step birth ---
function sized(n: number): number[] {
  return new Array<number>(n).fill(1);
}
show("dynamic 0", sized(0));
show("dynamic 3", sized(3));
show("dynamic 20", sized(20));
for (const bad of [-1, 1.5, 4294967296, NaN, Infinity]) {
  let evaluated = 0;
  try {
    const r = new Array<number>(bad).fill((evaluated = 1));
    console.log("no throw", bad, r.length);
  } catch (e) {
    console.log("range error", bad, e instanceof RangeError, (e as Error).message, evaluated);
  }
}
const any3: unknown = "3";
show("string argument", new Array<unknown>(any3 as number).fill(0));
show("fill start", new Array<number>(5).fill(1, 2));
show("fill start end", new Array<number>(5).fill(1, 1, 3));
show("fill negative start", new Array<number>(5).fill(1, -2));
const named = new Array<number>(4);
show("before fill", named);
named.fill(2);
show("named fill", named);
show("two arguments", new Array<number>(4, 5).fill(0));
show("no arguments", new Array<number>().fill(0));
class Stack extends Array<number> {
  top(): number {
    return this[this.length - 1];
  }
}
const stack = new Stack(4).fill(6);
console.log("subclass", stack.length, stack.top(), stack instanceof Stack, stack instanceof Array);
show("array call", Array<number>(4).fill(8));
show("array from", Array.from({ length: 3 }).fill(1));
show("literal fill", [1, 2, 3].fill(0));

// I32 at rest, the capacity reservation: an at-rest Array whose index stores
// may land past its birth length, inside a small proven hull `[lo, hi]`
// (`hi <= 2 * lo`, `hi <= 1024`), is born with `hi` slots, so a store past
// the length lengthens it where it stands and the storage never moves.
// Covered: a module Array lengthened by one and with a gap, `length` after
// each store, holes between the old length and the new one (`undefined`,
// through an int key, a fractional-typed key and a string key), a hole filled
// later, the last reserved slot, a loop over the lengthened Array,
// collections between the stores (young and promoted Arrays), a class field
// read through a method and a held base (the chess board shape, with a
// negative key that names a property), and an Array born per round.
// Negative controls that must stay double: a key that is no integer
// interval, a hull past
// twice the birth length, a hull past 1024 slots, and Arrays a generic
// reader touches (JSON, Object.keys, console.log).

function churn(n: number): number {
  // Allocation pressure, so collections run between the writes and reads.
  let keep: { v: number }[] = [];
  let s = 0;
  for (let i = 0; i < n; i++) {
    keep.push({ v: i });
    if (keep.length > 64) keep = [];
    s = (s + keep.length) | 0;
  }
  return s;
}

// A module Array: keys in [0, 15] over a birth length of 8.
const a = new Array<number>(8).fill(1);
function put(i: number, v: number): void {
  a[i & 15] = v & 0x7ffff000;
}
function get(i: number): number {
  return a[i & 15] ?? -1;
}
function raw(i: number): number | undefined {
  return a[i & 15];
}
console.log("birth", a.length, get(3), get(7), get(8), get(15), raw(8), raw(15));
put(3, 4096 * 5);
console.log("inside", a.length, get(3), raw(8));
put(8, 4096 * 6);
console.log("append", a.length, get(8), raw(9), raw(15));
put(12, 4096 * 7);
console.log("gap", a.length, get(12), raw(9), raw(10), raw(11), get(11), raw(13), raw(15));
let sum = 0;
let holes = 0;
for (let i = 0; i < a.length; i++) {
  const v = a[i];
  if (v === undefined) holes++;
  else sum = (sum + v) | 0;
}
console.log("loop", sum, holes, a.length);
churn(60000);
put(10, 4096 * 9);
console.log("filled", a.length, get(10), raw(9), raw(11), get(12));
churn(60000);
put(15, 4096 * 11);
console.log("last", a.length, get(15), raw(14), raw(13), get(12), get(8), get(3), get(0));
const keys: any[] = [0, 7, 8, 9, 12, 15, 16, 100, -1, 9.5, NaN, "9", "12", "length"];
for (const key of keys) console.log("key", String(key), a[key]);
let low = 0;
for (let i = 0; i < 8; i++) low = (low + (a[i] | 0)) | 0;
console.log("floor", low);

// The chess board shape: a class field, read through a method and a held
// base, stored through a method with a key that may be negative or past the
// birth length.
class Board {
  board: number[] = new Array<number>(128).fill(0);
  at(sq: number): number {
    return this.board[sq] ?? 0;
  }
  set(sq: number, v: number): void {
    this.board[sq] = v;
  }
  scan(from: number): number {
    let s = 0;
    let i = from;
    while (i < 128) {
      s = (s + this.at(i)) | 0;
      i += 1;
    }
    return s;
  }
  capture(to: number, color: number, v: number): void {
    const square = to - color * 16;
    this.set(square, v);
  }
}
const b = new Board();
for (let i = 0; i < 128; i++) b.set(i, (i * 3) & 15);
console.log("board", b.board.length, b.scan(0), b.scan(100), b.at(127), b.at(128), b.at(143));
b.capture(120, -1, 5);
console.log("past", b.board.length, b.at(136), b.at(135), b.at(128), b.scan(0));
b.capture(127, -1, 7);
console.log("edge", b.board.length, b.at(143), b.at(142), b.at(144), b.scan(64));
b.capture(3, 1, 9);
console.log("named", b.board.length, b.at(-13), b.at(-12), b.scan(0));
churn(60000);
console.log("kept", b.board.length, b.at(136), b.at(143), b.at(140), b.scan(0));

// An Array born per round, lengthened after a collection may have moved it.
let total = 0;
let lengths = 0;
for (let round = 0; round < 3000; round++) {
  const r = new Board();
  r.set(round & 127, round & 15);
  if ((round & 7) === 0) churn(40);
  if ((round & 3) === 1) r.capture((round & 15) + 112, -1, (round & 7) + 1);
  total = (total + r.scan(round & 63) + r.at(130) + r.at(141)) | 0;
  lengths = (lengths + r.board.length) | 0;
}
console.log("rounds", total, lengths);

// Negative controls: each stays a double Array.
const n = process.argv.length > 100 ? 1 : 0;
const unbounded = new Array<number>(4).fill(0);
function putAny(i: number, v: number): void {
  unbounded[i] = v;
}
putAny(2, 5);
putAny(n * 0.5 + 9, 6);
console.log("unbounded", unbounded.length, unbounded[2] | 0, unbounded[5], unbounded[9] | 0);
const twice = new Array<number>(4).fill(0);
function putTwice(i: number, v: number): void {
  twice[i & 15] = v;
}
putTwice(1, 3);
putTwice(13, 4);
console.log("twice", twice.length, twice[1] | 0, twice[7], twice[13] | 0);
const wide = new Array<number>(1000).fill(0);
function putWide(i: number, v: number): void {
  wide[(i & 1023) + 512] = v;
}
putWide(5, 8);
putWide(1023, 9);
console.log("wide", wide.length, wide[517] | 0, wide[1200], wide[1535] | 0);
const viaJson = new Array<number>(4).fill(0);
function putJson(i: number, v: number): void {
  viaJson[i & 7] = v;
}
putJson(6, 2);
console.log("json", JSON.stringify(viaJson), viaJson.length, viaJson[6] | 0);
const viaKeys = new Array<number>(4).fill(0);
function putKeys(i: number, v: number): void {
  viaKeys[i & 7] = v;
}
putKeys(6, 2);
console.log("keys", Object.keys(viaKeys).join(","), viaKeys.length, viaKeys[5]);
const viaLog = new Array<number>(4).fill(0);
function putLog(i: number, v: number): void {
  viaLog[i & 7] = v;
}
putLog(5, 2);
console.log("log", viaLog, viaLog.length);

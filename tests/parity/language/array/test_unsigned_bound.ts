// N10 (unsigned bound): an element read or store whose key is not tested for
// sign before the bound. `idx <u n` declines a negative key, and the miss arm
// sends it to the exact property-key path, so every key below must name what
// it names in JavaScript: -1 and 2^31 are named properties, -0 is element 0,
// NaN and 1.5 are named properties, a hole reads through the prototype (to
// undefined), and an empty array has no elements.

// (a) The rep-keyed tier, outside the floor: a field read call whose key is
// a native int (`k | 0`) over a range the floor does not cover.
class Board {
  board: number[];
  constructor(n: number) {
    this.board = new Array(n).fill(0);
    for (let i = 0; i < n; i++) this.board[i] = (i * 7) % 13;
  }
  at(i: number): number {
    return this.board[i] ?? -1;
  }
  ray(from: number, step: number): number {
    let s = 0;
    for (let k = 0; k < 40; k++) s += this.at((from + k * step) | 0);
    return s;
  }
}

const b = new Board(16);
console.log("ray up", b.ray(0, 1), "ray down", b.ray(5, -1), "ray far", b.ray(-20, 3));

class Grid {
  cells: number[];
  constructor(n: number) {
    this.cells = new Array(n).fill(0);
  }
  set(i: number, v: number): void {
    this.cells[i | 0] = v;
  }
  get(i: number): number {
    return this.cells[i | 0] ?? -1;
  }
}
const g = new Grid(8);
g.set(3, 99);
g.set(-4, 7); // a named property "-4", not an element
console.log("set", g.get(3), g.get(-4), g.get(8), g.cells.length, Object.keys(g.cells).join(","));

// (b) A local Array with a named "-1" property next to its elements.
const named: any = [10, 20, 30];
named[-1] = "minus one";
named["4294967295"] = "two to the 32 minus 1";
named["1.5"] = "one and a half";
named["NaN"] = "not a number";
function readInt(a: any, k: number): any {
  return a[k | 0];
}
function readNum(a: any, k: number): any {
  return a[k];
}
console.log("int", readInt(named, -1), readInt(named, 0), readInt(named, 2), readInt(named, 3));
console.log("int wrap", readInt(named, 2147483648), readInt(named, 4294967295), readInt(named, -0));
console.log(
  "num",
  readNum(named, -1),
  readNum(named, -0),
  readNum(named, NaN),
  readNum(named, 1.5),
  readNum(named, 2147483648),
  readNum(named, 4294967295),
  readNum(named, 2),
);
console.log("string key", named["-1"], named["0"], named["length"]);

// (c) A number[] read in a number context over keys that leave the array on
// both sides; the sum keeps the -1 default for every missing key.
function sumWindow(a: number[], lo: number, hi: number): number {
  let s = 0;
  for (let k = lo; k < hi; k++) s += a[k | 0] ?? -1;
  return s;
}
const dense = [1, 2, 3, 4, 5, 6, 7, 8];
console.log("window", sumWindow(dense, -5, 13), sumWindow(dense, 0, 8), sumWindow([], -3, 3));

// (d) Holes and length 0.
const holes: number[] = [1, , 3, , 5] as number[];
const empty: number[] = [];
let seen = "";
for (let k = -2; k < 7; k++) seen += String(holes[k | 0]) + "," + String(empty[k | 0]) + ";";
console.log("holes", seen);

// (e) Stores through a widened key: negative keys add named properties,
// in-range keys write elements, a key past the length grows the array.
const store: any[] = [0, 0, 0];
for (let k = -3; k < 5; k++) store[k | 0] = k * 10;
console.log("store", store.length, store.join("|"), store[-1], store[-3], Object.keys(store).join(","));
const wrap: any[] = [];
wrap[4294967295 | 0] = "wrapped"; // ToInt32 gives -1: a named property
wrap[2147483648 | 0] = "min"; // ToInt32 gives -2^31
console.log("store wrap", wrap.length, wrap[-1], wrap[-2147483648], Object.keys(wrap).join(","));

// (f) A fixed-length local read with a key range past its length.
const FIXED = [3, 1, 4, 1, 5, 9, 2, 6];
function fixedAt(k: number): number {
  return FIXED[(k & 15) - 4] ?? 100;
}
let fs = 0;
for (let k = 0; k < 32; k++) fs += fixedAt(k);
console.log("fixed", fs);

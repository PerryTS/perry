// Cold paths inside hot loops: the loop poll's slow call, the shared cold
// element read, store slow paths, a throw from a cold arm, the stack guard.
// Every loop keeps many numbers, integers and heap values alive across the
// cold arm, and every cold arm really runs (the negative controls of
// PERRY_COLD_PATHS: with the arm weighed as unlikely, a value that was not
// saved around it, or a root the collector did not see, changes a line).
// Every line is a value node prints too.

class P {
  x: number;
  y: number;
  tag: string;
  constructor(x: number, y: number, tag: string) {
    this.x = x;
    this.y = y;
    this.tag = tag;
  }
}

// 1. An allocating loop (it polls) with ten doubles, four integers and three
// heap values live across the back edge.
function poll(n: number): string {
  let a = 0.5, b = 1.25, c = 2.75, d = 3.5, e = 4.125, f = 5.0625, g = 6.5, h = 7.75, i2 = 8.25, j = 9.5;
  let k = 1, l = 2, m = 3, o = 4;
  let keep = new P(1, 2, "keep");
  const list: P[] = [];
  let text = "";
  for (let i = 0; i < n; i++) {
    const p = new P(i, a + b, "p" + (i & 7));
    if ((i & 63) === 0) list.push(p);
    a += b * 0.5; b += c * 0.25; c += d * 0.125; d += e * 0.5; e += f * 0.25;
    f += g * 0.125; g += h * 0.5; h += i2 * 0.25; i2 += j * 0.125; j += a * 1e-9;
    k = (k + l) | 0; l = (l ^ m) | 0; m = (m + o + i) | 0; o = (o * 3 + 1) & 0xffff;
    if ((i & 255) === 0) { keep = new P(keep.x + p.x, keep.y + 1, keep.tag); text += p.tag; }
    if (a > 1e12) { a = 0.5; b = 1.25; c = 2.75; d = 3.5; e = 4.125; f = 5.0625; g = 6.5; h = 7.75; i2 = 8.25; }
  }
  let sum = 0;
  for (const p of list) sum += p.x + p.y;
  return [a, b, c, d, e, f, g, h, i2, j].map((v) => v.toFixed(3)).join(" ") +
    " | " + [k, l, m, o].join(" ") + " | " + keep.x + " " + keep.y + " " + keep.tag +
    " | " + list.length + " " + sum.toFixed(3) + " " + text.length;
}
console.log("poll", poll(20000));

// 2. Element reads whose cold arm runs: a hole, past the end, a negative
// key, a fraction, a string key, a receiver that is not an array.
function reads(a: any, keys: any[], n: number): string {
  let s = 0.5, t = 1.5, u = 2.5, w = 3.5;
  let und = 0, str = 0;
  for (let i = 0; i < n; i++) {
    const key = keys[i % keys.length];
    const v = a[key];
    s += t * 0.5; t += u * 0.25; u += w * 0.125; w += 1;
    if (v === undefined) und++;
    else if (typeof v === "string") str += v.length;
    else if (typeof v === "number") s += v;
    else str += 100;
    if (s > 1e9) { s = 0.5; t = 1.5; u = 2.5; }
  }
  return s.toFixed(3) + " " + t.toFixed(3) + " " + u.toFixed(3) + " " + w + " " + und + " " + str;
}
const holey: any[] = [1, 2, , 4, 5.5, "six", , 8];
const KEYS: any[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, -1, 0.5, "3", "length", 100];
console.log("reads array", reads(holey, KEYS, 5000));
console.log("reads string", reads("hello world", KEYS, 5000));
console.log("reads object", reads({ 0: 7, 3: "x", length: 2 }, KEYS, 5000));

function pawnFiles(own: number[], n: number): number {
  let passed = 0, acc = 0.25;
  for (let i = 0; i < n; i++) {
    const file = i & 7;
    for (let f = file - 1; f <= file + 1; f++) {
      const v = own[f];
      acc += 0.5;
      if (v === undefined) passed += 1;
      else passed += v;
    }
  }
  return passed + acc;
}
console.log("pawn files", pawnFiles([0, 1, 0, 2, 0, 0, 3, 0], 4000));

// 3. Element stores whose slow path runs: growth, a hole, a kind change.
function stores(n: number): string {
  const a: any[] = [1, 2, 3];
  let s = 0.5, t = 1.5, u = 2.5;
  for (let i = 0; i < n; i++) {
    s += t * 0.5; t += u * 0.25; u += 1;
    if (i % 97 === 0) a[a.length + 2] = i;
    else if (i % 89 === 0) a[i % a.length] = "s" + i;
    else if (i % 83 === 0) a[i % a.length] = { i };
    else a[i % a.length] = s;
    if (s > 1e9) { s = 0.5; t = 1.5; }
  }
  let holes = 0, strings = 0, objects = 0, numbers = 0;
  for (let i = 0; i < a.length; i++) {
    const v = a[i];
    if (!(i in a)) holes++;
    else if (typeof v === "string") strings++;
    else if (typeof v === "object") objects++;
    else numbers++;
  }
  return a.length + " " + holes + " " + strings + " " + objects + " " + numbers + " " + s.toFixed(3) + " " + u;
}
console.log("stores", stores(6000));

// 4. A throw out of a cold arm, caught in the loop, with live values used in
// the handler and after it.
function throws(rows: any[], n: number): string {
  let s = 0.5, t = 1.5, u = 2.5, caught = 0, last = "";
  for (let i = 0; i < n; i++) {
    s += t * 0.5; t += u * 0.25; u += 1;
    try {
      const row = rows[i % rows.length];
      s += row.cells[i & 3];
    } catch (err) {
      caught++;
      last = (err as Error).constructor.name + ":" + i + ":" + t.toFixed(2);
      s -= 1;
    }
    if (s > 1e9) { s = 0.5; t = 1.5; }
  }
  return s.toFixed(3) + " " + t.toFixed(3) + " " + u + " " + caught + " " + last;
}
const ROWS: any[] = [{ cells: [1, 2, 3, 4] }, { cells: [5, 6, 7, 8] }, undefined, { cells: [9, 10, 11, 12] }, null];
console.log("throws", throws(ROWS, 3000));

function thrower(a: number[], i: number): number {
  if (i > a.length) throw new RangeError("past " + i);
  return a[i];
}
function throwsFromCallee(n: number): string {
  const a = [1.5, 2.5, 3.5];
  let s = 0.25, t = 0.75, caught = 0, und = 0;
  for (let i = 0; i < n; i++) {
    t += 0.5;
    try {
      const v = thrower(a, i % 6);
      if (v === undefined) und++; else s += v;
    } catch (err) {
      caught++;
      s += (err as Error).message.length + t;
    }
  }
  return s.toFixed(3) + " " + t + " " + caught + " " + und;
}
console.log("throws from callee", throwsFromCallee(3000));

// 5. The stack guard's arm: locals of every frame stay right after the
// RangeError unwinds through them.
function deep(n: number, a: number, b: number): number {
  const local = a * 0.5 + b * 0.25 + 1;
  if (n === 0) return local;
  return deep(n - 1, b, local) + 1;
}
console.log("deep ok", deep(2000, 1, 2).toFixed(3));
function overflow(a: number): number {
  const local = a + 0.5;
  return overflow(local) + local;
}
let guard = "none", before = 1.5, after = 2.5;
try {
  overflow(1);
} catch (err) {
  guard = (err as Error).constructor.name;
}
console.log("deep overflow", guard, before + after, deep(100, 1, 2).toFixed(3));

// 6. Heap values in locals across the poll and across a cold read, while
// the loop allocates: moved objects are read through their new address.
function moved(n: number): string {
  const board: any[] = [];
  for (let i = 0; i < 64; i++) board.push(i % 5 === 0 ? undefined : new P(i, i * 2, "sq" + i));
  let first = board[1] as P, s = 0.5, missing = 0;
  let name = "";
  for (let i = 0; i < n; i++) {
    const sq = board[(i * 7) % 70];
    const fresh = new P(i, s, "f" + (i % 3));
    if (sq === undefined) { missing++; if (i % 11 === 0) board[(i * 7) % 64] = fresh; }
    else { s += sq.x + sq.y * 0.5; name = sq.tag; }
    if (i % 13 === 0) first = fresh;
    s += first.x * 0.001;
    if (s > 1e9) s = 0.5;
  }
  return s.toFixed(3) + " " + missing + " " + name + " " + first.x + " " + first.tag;
}
console.log("moved", moved(20000));

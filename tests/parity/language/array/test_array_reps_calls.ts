// An array crossing calls (DESIGN-p4l1 §5.4 fixture 12): a direct call,
// `f.call`, a callback of `map`, a function stored and called through a
// table, a method, and a parameter that also receives something that is
// not an array.

function first(p: number[]): number {
  return p[0] ?? -1;
}
function total(p: number[]): number {
  let s = 0;
  for (let i = 0; i < 18; i++) {
    const k = i;
    s += p[k] ?? 100;
  }
  return s;
}

const a = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
const b = [2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30, 32, 34, 36];
console.log("direct", first(a), total(a), total(b));
console.log("call", first.call(null, a), total.call(undefined, b), total.apply(null, [a]));

const rows = [a, b, a];
console.log("map", rows.map(first).join(","), rows.map(total).join(","));
console.log("bound", first.bind(null, b)(), [a].map((r) => total(r))[0]);

// A table of functions chosen at run time.
const table: ((p: number[]) => number)[] = [first, total];
let s = 0;
for (let i = 0; i < 6; i++) s += table[i & 1](rows[i % 3]);
console.log("table", s);

// A method that reads its parameter, and one that keeps it.
class Reader {
  kept: number[] = [];
  at(p: number[], i: number): number {
    return p[i] ?? -1;
  }
  keep(p: number[]): void {
    this.kept = p;
  }
  sum(): number {
    return total(this.kept);
  }
}
const reader = new Reader();
reader.keep(b);
console.log("method", reader.at(a, 17), reader.at(a, 18), reader.sum());

// The same function also called with values that are no arrays.
function loose(p: any): unknown {
  return p[0];
}
console.log("loose", loose(a), loose("xyz"), loose({ 0: "o" }), loose(new Uint8Array([7])), loose.call(null, { length: 3 }));

// A short array passed where a longer one is read.
const short = [5, 6];
console.log("short", total(short), first(short), total([]), first([]));

// A returned array, read by the caller.
function make(n: number): number[] {
  const out = new Array(18).fill(n);
  out[3] = n * 2;
  return out;
}
console.log("returned", total(make(1)), make(2)[3], first(make(7)));

// Recursion carrying the array.
function walk(p: number[], i: number): number {
  return i >= p.length ? 0 : (p[i] ?? 0) + walk(p, i + 1);
}
console.log("recursive", walk(a, 0), walk(short, 0));

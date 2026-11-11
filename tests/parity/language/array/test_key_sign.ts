// Key signs: an element read or store whose key the whole-program facts
// prove an integer inside the int32 range takes no key normalizer, and one
// whose key is proven non-negative takes no sign test and has no
// property-key arm. Every key below must name what it names in JavaScript.
const many = process.argv.length > 100;

// (a) Keys the facts bound below at 0: a parameter joined over its callers,
// a class field, an element of an int Array, a sum of those.
function pick(xs: number[], k: number): number {
  return xs[k] ?? -1;
}
const base: number[] = [10, 20, 30, 40, 50, 60, 70, 80];
let sa = 0;
for (let i = 0; i < 8; i++) sa += pick(base, i);
sa += pick(base, many ? 3 : 5) + pick(base, 9) + pick(base, 200);
console.log("param", sa);

class Cursor {
  at: number = many ? 6 : 2;
  step: number = many ? 4 : 1;
  cells: number[] = [1, 2, 3, 4, 5, 6, 7, 8, 9];
  read(): number {
    return this.cells[this.at] ?? -1;
  }
  ahead(): number {
    return this.cells[this.at + this.step * 3] ?? -1;
  }
  far(): any {
    return this.cells[this.at + 40];
  }
  put(v: number): void {
    this.cells[this.at + this.step] = v;
  }
  grow(v: number): number {
    this.cells[this.at + 9] = v;
    return this.cells.length;
  }
}
const c = new Cursor();
c.put(33);
console.log("field", c.read(), c.ahead(), c.far(), c.cells[3], c.grow(77), c.cells[11], c.cells[10]);

const order: number[] = [3, 0, 2, 1, 3, 3, 0];
const values: any[] = ["a", "b", "c", "d"];
let chain = "";
for (let i = 0; i < order.length; i++) chain += values[order[i] ?? 0];
console.log("element key", chain);

// (b) Negative controls: a key that can be -1 names a property.
const named: any = [1, 2, 3];
named[-1] = "minus one";
named[-2147483648] = "min";
function probe(a: any, k: number): any {
  return a[k];
}
let pk = "";
for (let k = -1; k < 4; k++) pk += String(probe(named, k)) + ",";
console.log("can be -1", pk, probe(named, -2147483648), probe(base, -1), probe(base, -5));

class Signed {
  k: number = many ? 2 : -1;
  xs: number[] = [7, 8, 9];
  get(): any {
    return this.xs[this.k];
  }
  set(v: number): number {
    this.xs[this.k] = v;
    return this.xs.length;
  }
}
const sg = new Signed();
console.log("signed field", sg.get(), sg.set(5), sg.get(), Object.keys(sg.xs).join(","), sg.xs.join(","));

// (c) A key that is non-negative only behind a guard a `continue` leaves.
const deltas: number[] = [-3, 1, -1, 2, 0, -9, 4];
let guarded = 0;
let skipped = 0;
for (let i = 0; i < deltas.length; i++) {
  const k = deltas[i] ?? 0;
  if (k < 0) {
    skipped++;
    continue;
  }
  guarded += base[k] ?? -1;
}
let unguarded = "";
for (let i = 0; i < deltas.length; i++) {
  const k = deltas[i] ?? 0;
  unguarded += String(base[k]) + ",";
}
console.log("guard", guarded, skipped, unguarded);

// (d) A write in the guard's branch: the key may be negative after it.
let w = 0;
for (let i = 0; i < 6; i++) {
  let k = i - 2;
  if (k < 0) {
    k = k * 2;
  }
  w += base[k] ?? 100;
}
console.log("write in branch", w);

// (e) -0 is element 0; a fraction and NaN are named properties; 2^31 is an
// index past the end.
const zero = (many ? 3 : -4) * 0;
const half = (many ? 3 : 1) / 2;
const big = many ? 7 : 2147483648;
const notNumber = Number(many ? "1" : "x");
named["0.5"] = "half";
named["NaN"] = "nan";
console.log("edge", named[zero], Object.is(zero, -0), named[half], named[notNumber], named[big], base[zero]);
named[zero] = "first";
named[half] = "h2";
console.log("edge store", named[0], named["0.5"], named[big], named.length);

// (f) `at` counts a negative index from the end; untouched.
function atOf(xs: number[], k: number): any {
  return xs.at(k);
}
console.log("at", atOf(base, -1), atOf(base, 0), atOf(base, 7), atOf(base, -8), atOf(base, -9), atOf(base, 8));

// (g) Typed arrays: an integer key outside the view is undefined, a store
// there does nothing; untouched.
const ta = new Int32Array([5, 6, 7, 8]);
function taGet(t: Int32Array, k: number): any {
  return t[k];
}
function taSet(t: Int32Array, k: number, v: number): void {
  t[k] = v;
}
taSet(ta, -1, 99);
taSet(ta, 2, 70);
taSet(ta, 4, 99);
console.log("typed", taGet(ta, -1), taGet(ta, 0), taGet(ta, 2), taGet(ta, 3), taGet(ta, 4), ta.length);

// (h) Stores: a proven index past the length grows the Array with holes; a
// key that can be negative makes a property and leaves the length.
function put(xs: any[], k: number, v: any): number {
  xs[k] = v;
  return xs.length;
}
const grown: any[] = [1, 2];
console.log("store", put(grown, 1, "b"), put(grown, 5, "f"), grown[3], grown[5], 3 in grown);
const props: any[] = [1, 2];
function putAny(xs: any[], k: number, v: any): number {
  xs[k] = v;
  return xs.length;
}
console.log("store neg", putAny(props, -1, "n"), putAny(props, 0, "z"), props[-1], props[0], Object.keys(props).join(","));

// (i) An at-rest int Array read by a key only the facts bound.
class Ring {
  slots: number[] = new Array(16).fill(0);
  head: number = many ? 9 : 4;
  fill(): void {
    for (let i = 0; i < 16; i++) this.slots[i] = i * 3;
  }
  peek(): number {
    return this.slots[this.head] ?? -1;
  }
  next(): number {
    return this.slots[this.head + 11] ?? -1;
  }
  past(): number {
    return this.slots[this.head + 12] ?? -1;
  }
}
const ring = new Ring();
ring.fill();
console.log("at rest", ring.peek(), ring.next(), ring.past());

// (j) A loop of reads through each form, for the GC schedule arm.
let total = 0;
for (let round = 0; round < 2000; round++) {
  const fresh: number[] = [round, round + 1, round + 2, round + 3, round + 4, round + 5, round + 6, round + 7];
  total += pick(fresh, round & 7) + c.read() + ring.peek() + (probe(fresh, (round % 9) - 1) ?? 1000);
  const boxed: any[] = [{ v: round }, { v: 1 }];
  total += boxed[order[round % 7] & 1].v;
}
console.log("loop", total);

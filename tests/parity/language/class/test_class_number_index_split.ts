// An indexed method called with an index the site proves a Number selects
// its integer-index clone with an inline exact-integer test. Every Number
// that is not a nonnegative i32 (a fraction, -1, NaN, the infinities, 2**31)
// must take the generic body and read the property the key names; -0 is
// the index 0.
class Grid {
  private cells: number[] = [];
  private sparse: Record<string, number> = {};
  constructor() {
    for (let i = 0; i < 16; i++) this.cells.push(i * i);
  }
  at(index: number): number { return this.cells[index] ?? -1; }
  put(index: number, value: number): void { this.cells[index] = value; }
  key(index: number): string { return String(this.cells[index]); }
  size(): number { return this.cells.length; }
  tag(index: number, value: number): void { this.sparse[String(index)] = value; }
}

const g = new Grid();
const keys = [0, -0, 1, 3, 15, 16, 2.5, -1, NaN, Infinity, -Infinity, 2147483647, 2147483648, 4294967295];
let sum = 0;
for (const k of keys) {
  const doubled = k * 1;
  const v = g.at(doubled);
  sum += Number.isNaN(v) ? 0 : v;
  console.log(k, Object.is(k, -0), v, g.key(doubled));
}
for (const k of keys) {
  const i = k + 0;
  g.tag(i, 7);
  if (i >= 0 && i < 20) g.put(i, i * 3);
}
console.log(sum, g.size(), g.at(16), g.at(19), g.key(2.5));
const holes: number[] = [];
for (let i = 0; i < g.size(); i++) holes.push(g.at(i + 0));
console.log(holes.join(","));

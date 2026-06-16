// #5094: O(1) class-field raw-f64 layout fast path.
// Exercises the memory-corruption-risk cases the header-bit fast path must
// handle: (a) downgrade a number field via an `any` alias then read it back,
// (b) a mixed pointer+number class, (c) GC churn mid-access.

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

const p = new Point(1, 2);

// Hot read/write loop — drives the fast path.
let sum = 0;
for (let i = 0; i < 200000; i++) {
  p.x = p.x + 1;
  sum = sum + p.y;
}
console.log("sum:" + sum);
console.log("px:" + p.x);

// (a) Downgrade x to a string via an `any` alias. The canonical bit must clear
// so the subsequent read does NOT reinterpret the string pointer as a double.
const a: any = p;
a.x = "downgraded";
console.log("after-downgrade-x:" + p.x);
console.log("typeof-x:" + typeof a.x);
// y is untouched and must still read as a number.
console.log("after-downgrade-y:" + (p.y + 40));

// (b) Mixed class: number + pointer fields.
class Mixed {
  n: number;
  label: string;
  constructor(n: number, label: string) {
    this.n = n;
    this.label = label;
  }
}
const m = new Mixed(7, "tag");
let acc = 0;
for (let i = 0; i < 200000; i++) {
  m.n = m.n + 2;
  acc = acc + m.n;
}
console.log("mixed-n:" + m.n);
console.log("mixed-label:" + m.label);
console.log("mixed-acc-mod:" + (acc % 1000));

// (c) GC churn: allocate many Points while reading fields of a survivor.
const survivor = new Point(100, 200);
let g = 0;
for (let i = 0; i < 100000; i++) {
  const tmp = new Point(i, i + 1);
  g = g + tmp.x + survivor.x;
}
console.log("survivor-x:" + survivor.x);
console.log("g-mod:" + (g % 7));

// VR-0 (DESIGN-p4l4 finding 14, §13 risk 6): an object whose only escape is
// to a JSON.stringify replacer. The replacer runs with (key, value) and
// `this` = the holder for every value serialized, so it may store into the
// object; a later read of the "readonly" field must see the store.

const p = { n: 1, s: "a" };
const holder = { p };
let seen = 0;
const out = JSON.stringify(holder, function (key: string, value: any) {
  if (key === "p") {
    value.n = "two";
    value.s = 3;
    seen++;
  }
  return value;
});
console.log(out, seen);
console.log(p.n, typeof p.n, p.s + 1);
let acc: any = 0;
for (let i = 0; i < 3; i++) acc = acc + p.n;
console.log(acc);

// The same through `this` (the holder), on a class instance.
class Point {
  x: number;
  constructor() {
    this.x = 1;
  }
}
const q = new Point();
const box = { q, tag: "box" };
JSON.stringify(box, function (this: any, key: string, value: any) {
  if (key === "tag") this.q.x = "seven";
  return value;
});
console.log(q.x, typeof q.x, q.x * 2);

// A replacer that only reads (control): the object is unchanged.
const r = { n: 5 };
console.log(JSON.stringify({ r }, (k, v) => v), r.n * 2);

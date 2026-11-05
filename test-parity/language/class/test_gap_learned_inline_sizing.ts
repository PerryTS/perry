// Learned per-class inline sizing: the first instance of a dynamic/function
// constructor that overflows records its field high-water mark; later `new`
// pre-sizes so all fields land inline. This must be behavior-transparent —
// construction, field reads/writes, Object.keys, and JSON.stringify of a wide
// (>=12-field, so field_count clears internal shape gates) instance must match
// node exactly, across many instances (instance 1 overflows, 2+ are pre-sized).
function Wide(this: any, seed: number) {
  this.a = seed; this.b = 1; this.c = 2; this.d = 3; this.e = 4; this.f = 5;
  this.g = 6; this.h = 7; this.i = 8; this.j = 9; this.k = 10; this.l = 11;
  this.m = 12; this.n = 13; this.o = 14; this.p = 15; this.q = 16; this.r = 17;
  this.child = null;
}
const out: string[] = [];
const kept: any[] = [];
for (let i = 0; i < 5000; i++) {
  const w: any = new (Wide as any)(i);
  w.child = { x: i, url: "https://example.com/" + i + "?q=1" };
  w.r = w.r + i;                         // overflow-field overwrite
  if (i % 1000 === 0) kept.push(w);
}
const w0 = kept[0];
out.push("keys=" + Object.keys(w0).length);
out.push("a=" + w0.a + " r=" + w0.r + " child.x=" + w0.child.x);
out.push("json=" + JSON.stringify(w0));
out.push("jsonArr=" + JSON.stringify(kept.map((k) => ({ a: k.a, r: k.r }))));
let sum = 0;
for (const k of kept) sum += k.a + k.b + k.r + k.child.x;
out.push("sum=" + sum);
console.log(out.join("\n"));

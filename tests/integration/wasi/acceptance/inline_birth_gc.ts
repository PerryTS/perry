// Emitted Eden births on wasm32 (class bump, fixed layout, array literal, empty
// array, closure, function-constructor family) write the target's header image:
// GcHeader, class id + layout word, the ILP32 padding word and the meta pointer
// at 12, holes in the slots. Objects kept across many nursery collections are
// read back by the runtime, grown (meta/spill), and enumerated.
class P {
  x: number;
  y: number;
  name: string;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
    this.name = "p" + (x % 7);
  }
}
class Q {
  constructor(a: number) {
    (this as any).a = a;
    (this as any).b = a + 1;
  }
}
function F(this: any, a: number) {
  this.a = a;
  this.b = a * 2;
}
const keep: any[] = [];
let sum = 0;
for (let i = 0; i < 300000; i++) {
  const p = new P(i, i + 1);
  const q: any = new Q(i);
  const o: any = { a: i, b: [i, i + 1], c: "s" + (i & 255) };
  const f = () => p.x + o.a;
  const e: number[] = [];
  sum += f() + o.b[1] - p.y + e.length + q.b;
  if (i % 1000 === 0) {
    o.extra = i;
    (p as any)["k" + (i % 3)] = i;
    q.late = -i;
    e.push(i);
    keep.push(p, q, o, f, e, new (F as any)(i));
  }
}
let check = 0;
let keys = "";
for (let k = 0; k < keep.length; k += 6) {
  const p = keep[k], q = keep[k + 1], o = keep[k + 2], f = keep[k + 3], e = keep[k + 4], g = keep[k + 5];
  check += p.x + p.y + p.name.length + q.a + q.b + q.late + o.a + o.b[0] + o.b[1] + o.c.length;
  check += o.extra + f() + e[0] + e.length + g.a + g.b;
  if (k === 6) keys = [Object.keys(p), Object.keys(q), Object.keys(o), Object.keys(g)].join("|");
  delete o.extra;
  check += Object.keys(o).length;
}
console.log(sum, check, keep.length / 6);
console.log(keys);
console.log(JSON.stringify(keep[keep.length - 4]), keep[0] instanceof P, keep[1] instanceof Q);

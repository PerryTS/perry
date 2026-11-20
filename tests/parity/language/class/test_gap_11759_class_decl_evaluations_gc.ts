// #11759: later evaluations of a class declaration are heap class objects.
// Create and drop many of them under allocation pressure (moving minors and
// full collections) and check each stays intact: its statics, prototype,
// captured name and instances.

function make(n: number) {
  class Item {
    static serial = n;
    static all: Item[] = [];
    v: number;
    label: string;
    constructor(v: number) { this.v = v; this.label = "item" + v; Item.all.push(this); }
    twice() { return this.v * 2 + Item.serial; }
  }
  return Item;
}

function churn(k: number): number {
  let s = 0;
  for (let i = 0; i < 200; i++) { const o = { a: i, b: "s" + i, c: [i, k] }; s += o.c.length; }
  return s;
}

const kept: any[] = [];
let ok = true;
for (let i = 0; i < 400; i++) {
  const C: any = make(i);
  const a = new C(i), b = new C(i + 1);
  churn(i);
  if (a.twice() !== i * 2 + i || b.label !== "item" + (i + 1) || C.all.length !== 2) ok = false;
  if (!(a instanceof C) || (kept.length > 0 && a instanceof kept[kept.length - 1])) ok = false;
  if (i % 40 === 0) kept.push(C);
}
churn(1);
const sums = kept.map((C: any) => C.serial + ":" + C.all.length + ":" + new C(1).twice()).join(",");
console.log("ok", ok, kept.length);
console.log("kept", sums);
console.log("distinct", new Set(kept).size, kept[0] === make(0));

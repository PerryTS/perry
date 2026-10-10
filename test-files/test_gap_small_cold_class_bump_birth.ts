// Indirect factories retain instance identity, inherited slots, and birth values.
declare function gc(): void;
function collect(): void { if (typeof gc === "function") gc(); }
class Base {
  x: number;
  label: string;
  constructor(x: number) { this.x = x; this.label = "item-" + x; }
  value() { return this.x; }
}
class Point extends Base {
  y: number;
  child: any;
  constructor(x: number) { super(x); this.y = x + 1; this.child = { n: x * 2 }; }
}
function make(x: number) { return new Point(x); }
const factories: any[] = [make];
const first = factories[0](7);
const second = factories[0](7);
collect();
console.log(first === second, first instanceof Point, first instanceof Base);
console.log(first.value(), first.label, first.y, first.child.n);
let sum = 0;
let last: any = null;
for (let i = 0; i < 50000; i++) {
  const p = factories[i % factories.length](i);
  if (i % 10000 === 0) collect();
  if (p !== last) sum += p.value() + p.y + p.child.n;
  last = p;
}
console.log(sum, last.label, first.label, first.child.n);

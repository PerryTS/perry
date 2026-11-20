// A Number has one encoding in every JS value (the raw double). Fields the
// whole-program facts prove only hold Numbers read as plain doubles; these
// cases pin node's results for the edge values, every write route and a
// field that may hold a BigInt (which must stay generic).
class P {
  v;
  constructor(v) {
    this.v = v;
  }
  scaled(k: number): number {
    return this.v * k;
  }
}
class Q extends P {
  w;
  constructor(v, w) {
    super(v);
    this.w = w;
  }
  sum(): number {
    return this.v + this.w;
  }
}
const ints = [4, 5, 6];
let acc = 0;
for (let i = 0; i < 10; i++) acc = (acc + 3) | 0;
const values = [
  -0, NaN, 2147483647, -2147483648, 2147483648, 5, 1.5,
  ints.length, acc, (acc * 1000000) | 0, Math.floor(7.9), "abc".length,
];
const out: string[] = [];
for (const value of values) {
  const p = new P(value);
  const r = p.scaled(1.5);
  out.push(String(r) + (Object.is(p.v, -0) ? "(-0)" : "") + (Object.is(p.v, value) ? "" : "!is"));
}
console.log(out.join(","));

// Object.assign and a bracket key write the same field.
const a = new P(0);
Object.assign(a, { v: 7 });
console.log(a.scaled(2), Object.is(a.v, 7));
const b = new P(0);
b["v"] = -2147483648;
console.log(b.scaled(-1), Object.is(b.v, -2147483648));

// A subclass field.
const q = new Q(3, 2147483647);
console.log(q.sum(), q.scaled(2), Object.is(q.w, 2147483647));

// A JSON round trip.
const parsed = JSON.parse(JSON.stringify({ v: 42, w: -1 }));
const c = new Q(parsed.v, parsed.w);
console.log(c.sum(), c.scaled(0.5), Object.is(c.v, 42), JSON.stringify(c));

// A field that may hold a BigInt stays generic.
class R {
  n;
  constructor(n) {
    this.n = n;
  }
  twice() {
    return this.n * (typeof this.n === "bigint" ? 2n : 2);
  }
}
const rs = [new R(21), new R(21n), new R(-0)];
console.log(rs.map((r) => String(r.twice())).join(","), Object.is(rs[2].twice(), -0));

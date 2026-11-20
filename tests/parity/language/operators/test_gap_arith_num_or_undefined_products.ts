// `-` `*` `/` over fields that hold a Number or `undefined` (typed-array reads
// past the end), and the `+` of their products: undefined must become NaN,
// never leak as a value, and `-0` must survive.
class V3 {
  x: any; y: any; z: any;
  constructor(x: any, y: any, z: any) { this.x = x; this.y = y; this.z = z; }
  fromArray(a: Float64Array, i: number): V3 { this.x = a[i]; this.y = a[i + 1]; this.z = a[i + 2]; return this; }
  dot(v: V3): any { return this.x * v.x + this.y * v.y + this.z * v.z; }
  mix(v: V3, e: number): any { return this.x * v.x + this.y * v.y + e; }
  diff(v: V3): any { return (this.x - v.x) / (this.z - v.y); }
  late(o: any): any { return this.x + this.y + o.v; }
  sq(): any { return this.x * this.x - this.y * this.z + this.z / this.y; }
}
const off = Number(process.argv[2] ?? String(Math.floor(Math.random() * 0.5)));
const buf = new Float64Array([1.5, -2, 3, 0.25, -0, 7]);
const a = new V3(0, 0, 0).fromArray(buf, off);
const b = new V3(0, 0, 0).fromArray(buf, off + 3);
const c = new V3(0, 0, 0).fromArray(buf, off + 4);
const d = new V3(0, 0, 0).fromArray(buf, off + 9);
const z = new V3(0, 0, 0).fromArray(buf, off + 3);
z.x = -0; z.y = 0;
const out: string[] = [];
const g = { get v() { out.push("get"); return 2; } };
const pairs: [V3, V3][] = [[a, b], [a, c], [c, a], [d, a], [b, b], [z, z], [d, d]];
for (const [p, q] of pairs) {
  const r = p.dot(q);
  const m = p.diff(q);
  out.push(String(r), String(Object.is(r, NaN)), String(r !== r), typeof r);
  out.push(String(m), String(Object.is(m, -0)), String(p.mix(q, 0.5)), String(p.late(g)), String(p.sq()));
}
console.log(out.join(" "));

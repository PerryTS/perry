// Element reads the facts prove a Number (or a Number or `undefined`: a read
// past the end) as operands of `*` `-` `/` and of the `+` of their products:
// no dynamic helper, `undefined` becomes NaN (never a leaked value), `-0` kept.
class M4 {
  elements: number[];
  constructor() { this.elements = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]; }
  scale(s: number): M4 { const te = this.elements; for (let i = 0; i < 16; i++) te[i] = te[i] * s - i / 4; return this; }
  neg(): M4 { const te = this.elements; for (let i = 0; i < 16; i++) te[i] = -te[i]; return this; }
  maxAxis(): number {
    const te = this.elements;
    const a = te[0] * te[0] + te[1] * te[1] + te[2] * te[2];
    const b = te[4] * te[4] + te[5] * te[5] + te[6] * te[6];
    return Math.sqrt(Math.max(a, b));
  }
  mixed(k: number): number { const te = this.elements; return te[0] * te[1] + te[k] - te[2] / te[3]; }
}
class P3 {
  x: number; y: number; z: number;
  constructor(x: number, y: number, z: number) { this.x = x; this.y = y; this.z = z; }
  apply(m: M4): P3 {
    const x = this.x, y = this.y, z = this.z;
    const e = m.elements;
    const w = 1 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    this.x = (e[0] * x + e[4] * y + e[8] * z + e[12]) * w;
    this.y = (e[1] * x + e[5] * y + e[9] * z + e[13]) * w;
    this.z = (e[2] * x + e[6] * y + e[10] * z + e[14]) * w;
    return this;
  }
}
const s = Number(process.argv[2] ?? String(Math.floor(Math.random() * 0.5)));
const out: string[] = [];
const ms = [new M4(), new M4().scale(s), new M4().scale(s).neg(), new M4().neg()];
for (const m of ms) {
  const p = new P3(1, s, -0).apply(m);
  const r = m.maxAxis();
  const q = m.mixed(s > 5 ? 40 : 1);
  out.push(String(p.x), String(p.y), String(p.z), String(Object.is(p.z, -0)), String(r), String(q), String(q !== q));
  out.push(String(Object.is(m.elements[1] * m.elements[0], -0)));
}
console.log(out.join(" "));

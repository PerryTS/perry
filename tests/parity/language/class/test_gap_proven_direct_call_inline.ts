// RED: small methods with a cold throw path, called directly on facts-proven receivers in a hot loop
class V { x: number; y: number; z: number;
  constructor(x = 0, y = 0, z = 0) { this.x = x; this.y = y; this.z = z; }
  copy(v: V) { this.x = v.x; this.y = v.y; this.z = v.z; return this; }
  dot(v: V) { return this.x * v.x + this.y * v.y + this.z * v.z; } }
class S { c: V; r: number;
  constructor(c = new V(), r = -1) { this.c = c; this.r = r; }
  copy(s: S) { this.c.copy(s.c); this.r = s.r; return this; }
  contains(p: V) { const dx = p.x - this.c.x, dy = p.y - this.c.y, dz = p.z - this.c.z; return dx * dx + dy * dy + dz * dz <= this.r * this.r; } }
class R { o: V; d: V;
  constructor(o = new V(), d = new V(0, 0, -1)) { this.o = o; this.d = d; }
  copy(r: R) { this.o.copy(r.o); this.d.copy(r.d); return this; }
  at(t: number, out: V) { out.x = this.o.x + this.d.x * t; out.y = this.o.y + this.d.y * t; out.z = this.o.z + this.d.z * t; return out; }
  recast(t: number) { this.o.copy(this.at(t, _v)); return this; } }
const _s = new S(); const _r = new R(); const _v = new V();
function step(src: S, ray: R, k: number): number {
  _s.copy(src); _r.copy(ray).recast(k * 0.001);
  return _s.contains(_r.o) ? 1 : 0;
}
const n = Number(process.argv[2] || "7");
const src = new S(new V(n, 1, 2), n * 0.5); const ray = new R(new V(0, 0, 0), new V(0.6, 0.8, 0));
let hits = 0; let acc = 0;
for (let i = 0; i < 2000000; i++) { hits += step(src, ray, i % 997); acc += _r.o.x; }
console.log(hits, acc.toFixed(3), _s.r, _r.o.y.toFixed(4));

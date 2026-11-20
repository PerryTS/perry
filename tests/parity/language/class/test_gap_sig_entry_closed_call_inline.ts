// RED: a site proven to run one body enters it through its typed-signature entry (M$sig)
class V3 { x: number; y: number; z: number;
  constructor(x = 0, y = 0, z = 0) { this.x = x; this.y = y; this.z = z; }
  set(x: number, y: number, z: number) { this.x = x; this.y = y; this.z = z; return this; }
  distSq(v: V3) { const dx = this.x - v.x, dy = this.y - v.y, dz = this.z - v.z; return dx * dx + dy * dy + dz * dz; }
  dist(v: V3) { return Math.sqrt(this.distSq(v)); } }
class Bx { min: V3; max: V3;
  constructor() { this.min = new V3(-1, -2, -3); this.max = new V3(1, 2, 3); }
  center(t: V3) { return t.set((this.min.x + this.max.x) * 0.5, (this.min.y + this.max.y) * 0.5, (this.min.z + this.max.z) * 0.5); }
  grow(p: V3) { this.max.set(Math.max(this.max.x, p.x), Math.max(this.max.y, p.y), Math.max(this.max.z, p.z)); return this; } }
const k = Number(process.argv[2] || "7") + String(Math.random()).length * 0;
const b = new Bx(); const c = new V3(); const p = new V3(k, 0, 0);
let s = 0;
for (let i = 0; i < 2000000; i++) {
  p.set((i % 97) * 0.01 * k, (i % 13) * 0.1, -(i % 7));
  b.grow(p); b.center(c);
  s += c.x + c.y * 0.5 + c.z + p.dist(c);
}
console.log("sum", s.toFixed(6), c.x, c.y, c.z);

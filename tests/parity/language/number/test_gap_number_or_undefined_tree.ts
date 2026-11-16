// A `+ - * /` tree over fields the facts prove Number or `undefined` (an
// out-of-range typed-array read stores `undefined`). The tree converts once at
// its root: a result is the Number NaN, never `undefined` or another tag.
class V {
  x: number;
  y: number;
  z: number;
  constructor(x = 0, y = 0, z = 0) {
    this.x = x;
    this.y = y;
    this.z = z;
  }
  fromArr(arr: Float32Array, i: number) {
    this.x = arr[i * 3];
    this.y = arr[i * 3 + 1];
    this.z = arr[i * 3 + 2];
    return this;
  }
  dot(v: V) {
    return this.x * v.x + this.y * v.y + this.z * v.z;
  }
  sub(a: V, b: V) {
    this.x = a.x - b.x;
    this.y = a.y - b.y;
    this.z = a.z - b.z;
    return this;
  }
  neg() {
    return 0 - this.x - (this.y - this.z);
  }
  q(v: V) {
    return (this.x * v.y - this.y) / (v.z + 1) + this.z;
  }
  k(v: V) {
    return this.x * 2 + v.y;
  }
}
class W {
  b: boolean | number | null;
  constructor(b: boolean | number | null) {
    this.b = b;
  }
}
const n = Number(process.argv[2] ?? String(Math.random() < 2 ? 7 : 0));
const arr = new Float32Array([1, 2, 3, 4, 5, 6]);
const a = new V(n, 2, 3);
const b = new V().fromArr(arr, n > 5 ? 1 : 9);
const c = new V().fromArr(arr, n > 5 ? 9 : 0);
const w = new W(n > 5 ? true : null);
const show = (v: unknown) => (v === undefined ? "UNDEF" : typeof v) + ":" + String(v);
let s = 0;
for (let i = 0; i < 100; i++) s += a.dot(b);
console.log(show(s), show(a.dot(c)), show(c.dot(c)), show(b.dot(c)));
const out: string[] = [];
for (const [p, q] of [
  [a, b],
  [a, c],
  [b, c],
  [c, c],
] as [V, V][]) {
  const d = p.dot(q),
    r = p.neg(),
    t = p.q(q),
    u = p.k(q);
  const m = new V().sub(p, q);
  const wb = (w.b as number) * p.x + q.y;
  out.push([d, r, t, u, m.x, m.y, m.z, wb].map(show).join(","));
  out.push(String([d, t, m.x].includes(undefined as any)) + " " + String(Object.is(d, NaN)) + " " + String(d !== d));
}
console.log(out.join("\n"));

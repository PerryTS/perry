// A dot product over fields the facts prove Number or `undefined` (set from a
// typed-array read that may be past the end): one conversion at the root of
// the + * tree; a result is the Number NaN, never `undefined`.
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
}
const n = Number(process.argv[2] ?? String(Math.random() < 2 ? 7 : 0));
const arr = new Float32Array([1, 2, 3, 4, 5, 6]);
const a = new V(n, 2, 3);
const b = new V().fromArr(arr, n > 5 ? 1 : 9);
const c = new V().fromArr(arr, n > 5 ? 9 : 0);
const show = (v: unknown) => (v === undefined ? "UNDEF" : typeof v) + ":" + String(v);
let s = 0;
for (let i = 0; i < 100; i++) s += a.dot(b);
const d = a.dot(c), e = c.dot(c), f = b.dot(a);
console.log(show(s), show(d), show(e), show(f), String(d !== d), String(e === undefined), show(b.x), show(c.x));

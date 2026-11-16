// An object with a user next is iterated through its own protocol.
const it = { i: 0, [Symbol.iterator]() { return this; }, next() { this.i++; return this.i > 3 ? { value: undefined, done: true } : { value: this.i * 10, done: false }; } };
let s = 0;
for (const v of it as any) s += v;
console.log("user-next", s);
function* g(): Generator<number> { yield 1; yield 2; }
const o = g();
const os: number[] = [];
(o as any).next = function () { const k = ((this as any).k = ((this as any).k || 0) + 1); return k > 2 ? { value: undefined, done: true } : { value: 7 * k, done: false }; };
for (const v of o) os.push(v);
console.log("own-next", os.join(","));

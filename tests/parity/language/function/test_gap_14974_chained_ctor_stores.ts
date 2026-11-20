// A chained `this.a = this.b = v` in a function constructor adds the inner
// key first; every instance keeps that key order through deletes, re-adds
// and an inherited setter on a chained key.
function F(this: any, t: number) { this.tag = t; this.a = this.b = this.c = null; this.n = 0; }
const f = new F(1);
console.log(Object.keys(f).join(','), JSON.stringify(f));
f.b = 2; delete f.a; f.a = 3;
console.log(Object.keys(f).join(','), JSON.stringify(f));
for (let i = 0; i < 3; i++) { const o = new F(i); console.log(Object.keys(o).join(','), o.tag, o.a, o.n); }
function G(this: any, x: number) { this.p = this.q = x; }
const log: string[] = [];
Object.defineProperty(G.prototype, 'q', { set(v) { log.push('set q ' + v); }, configurable: true });
const g = new G(5);
console.log(Object.keys(g).join(','), g.p, g.q, log.join('|'));
function H(this: any, k: string) { this[k] = this.z = 1; this.y = 2; }
console.log(Object.keys(new H('w')).join(','));
let made = 0;
for (let i = 0; i < 20000; i++) { const o = new F(i); if (o.c === null && o.tag === i) made++; }
console.log('made', made);

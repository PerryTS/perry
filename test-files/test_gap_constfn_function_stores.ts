// Function-valued stores (`this.k = F`, `this.cb = fn`, `o.m = function(){}`):
// a ConstFn lane names the body, the slot holds each object's own closure.
// The store site caches "a closure of body B moves shape S to S'" and checks
// the stored closure's body on every hit; anything else must take the slow
// path (deprecating the lane) and every later call must see the right body.
import { Worker } from 'node:worker_threads';

// 1. The same body stored repeatedly by a plain function constructor.
function P(this: any, v: number) { this.k = P; this.v = v; }
let same = 0;
const ps: any[] = [];
for (let i = 0; i < 3000; i++) { const x: any = new (P as any)(i); if (x.k === P) same++; ps.push(x); }
console.log('same body', same, ps[2999].v, ps[7].k === P, Object.keys(ps[5]).join(','));

// 2. Different closures of one body: each object keeps its own environment.
function mk(i: number) { return function (this: any) { return i * 10 + this.n; }; }
const fs: any[] = []; for (let i = 0; i < 7; i++) fs.push(mk(i));
function H(this: any, f: any, n: number) { this.cb = f; this.n = n; }
let sum = 0, own = 0;
const hs: any[] = [];
for (let i = 0; i < 3000; i++) {
  const x: any = new (H as any)(fs[i % 7], i % 3);
  if (x.cb === fs[i % 7]) own++;
  sum += x.cb();
  hs.push(x);
}
console.log('closures', own, sum, hs[10].cb(), hs[11].cb());

// 3. Another body at a warm site, then an overwrite with another body.
function A(this: any) { return 'A' + this.t; }
function B(this: any) { return 'B' + this.t; }
function Q(this: any, f: any, t: number) { this.m = f; this.t = t; }
const qs: any[] = [];
let calls = '';
for (let i = 0; i < 2000; i++) { const x: any = new (Q as any)(A, i); qs.push(x); if (i % 500 === 0) calls += x.m(); }
for (let i = 0; i < 6; i++) { const x: any = new (Q as any)(i % 2 ? B : A, i); qs.push(x); calls += x.m(); }
console.log('other body', calls);
const old: any = qs[3];
old.m = B;                                  // deprecate the lane on an existing object
console.log('overwrite', old.m(), qs[4].m(), qs[2004].m(), qs[2005].m());
for (let i = 0; i < 4; i++) { const x: any = new (Q as any)(A, 100 + i); calls = x.m() + (x.m === A); }
console.log('after deprecation', calls, new (Q as any)(B, 9).m());

// 4. A method called through the store; a fresh closure of one body per object.
function getV(this: any) { return this.v * 2; }
function R(this: any, v: number) { this.get = getV; this.v = v; }
let rs = 0; for (let i = 0; i < 2000; i++) rs += new (R as any)(i).get();
const lit: any[] = [];
for (let i = 0; i < 2000; i++) { const o: any = { n: i }; o.m = function (this: any) { return this.n + 1; }; lit.push(o); }
let ls = 0; for (const o of lit) ls += o.m();
console.log('methods', rs, ls, lit[0].m === lit[1].m);

// 5. Overwrites of an existing function-valued property.
function G1() { return 1; }
function G2() { return 2; }
const o: any = { a: 1 }; o.cb = G1;
let os = 0; for (let i = 0; i < 3000; i++) { o.cb = G1; os += o.cb(); }
const p: any = { b: 1 }; p.cb = fs[0];
for (let i = 0; i < 3000; i++) { p.cb = fs[i % 7]; os += p.cb.call({ n: 1 }); }
o.cb = G2; os += o.cb();
for (let i = 0; i < 10; i++) { o.cb = i % 2 ? G1 : G2; os += o.cb(); }
o.cb = 7; console.log('overwrites', os, o.cb, typeof p.cb);

// 6. Values that are not a plain closure of the body: a number, a bound
// function, an arrow capturing `this`, null, a string, an object.
function K(this: any, f: any) { this.k = f; }
for (let i = 0; i < 1000; i++) new (K as any)(G1);
const vals: any[] = [5, G1.bind(null), null, 'G1', { call: 1 }, undefined, G2, G1];
console.log('values', vals.map((v) => { const x: any = new (K as any)(v); return typeof x.k + ':' + (typeof x.k === 'function' ? x.k() : String(x.k)); }).join(' '));
function W(this: any) { this.cb = () => this.n; this.n = 4; }
let ws = 0; for (let i = 0; i < 500; i++) ws += new (W as any)().cb();
console.log('arrow', ws);

// 7. A setter defined on the prototype after the site is warm intercepts the add.
function S(this: any) { this.k = G1; this.d = 1; }
for (let i = 0; i < 1000; i++) new (S as any)();
let seen = 0;
Object.defineProperty(S.prototype, 'k', { set(v: any) { seen += v(); }, get() { return G2; }, configurable: true });
const s: any = new (S as any)();
console.log('setter', seen, s.k(), Object.prototype.hasOwnProperty.call(s, 'k'));

// 8. A worker running the same stores.
const worker = new Worker(new URL('./_helpers/constfn_store_worker.ts', import.meta.url));
worker.on('message', (m: string) => {
  console.log('worker', m);
  worker.terminate().then(() => process.exit(0));
});
setTimeout(() => process.exit(2), 10000);

import { parentPort } from 'node:worker_threads';
function F(this: any) { return 3 + this.v; }
function Fb(this: any) { return 4 + this.v; }
function C(this: any, f: any, v: number) { this.k = C; this.m = f; this.v = v; }
function mk(i: number) { return function () { return i; }; }
let s = 0;
for (let i = 0; i < 3000; i++) { const x: any = new (C as any)(i % 1000 === 999 ? Fb : F, i % 5); s += x.m() + (x.k === C ? 1 : 0); }
const o: any = { a: 1 }; o.cb = mk(0);
for (let i = 0; i < 3000; i++) { o.cb = mk(i % 4); s += o.cb(); }
parentPort!.postMessage(String(s));

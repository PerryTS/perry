// SEGV1: a method call or accessor the facts prove runs one body, on a
// local built with `new` in the same function that scalar replacement keeps
// in frame slots: the call stays the slots' inlined body (no heap `this`).
class A {
  v: number;
  constructor(v: number) { this.v = v; }
  m(x: number): number { return this.v + x; }
}
class B extends A {
  w: number;
  constructor(v: number) { super(v); this.w = v * 2; }
  n(x: number): number { return this.w * x; }
}
class G {
  v: number;
  constructor(v: number) { this.v = v; }
  get d(): number { return this.v * 3; }
  set d(x: number) { this.v = x; }
}
class M {
  f: number;
  constructor(f: number) { this.f = f; }
  inc(x: number): number { this.f = this.f + x; return this.f; }
}
let counter = 0;
function bump(): number { counter++; return counter; }
const arg = process.argv[2] ?? "0";
function plainConst(): void { const a0 = new A(Number(arg) + 10); console.log("plainConst", a0.m(7)); }
function plainBump(): void { const a0 = new A(Number(arg) + 10); console.log("plainBump", a0.m(bump())); }
function sub(): void { const b = new B(Number(arg) + 1); console.log("sub", b.m(bump()), b.n(2)); }
function acc(): void { const g = new G(Number(arg) + 2); console.log("acc", g.d); g.d = 5; console.log("acc2", g.d, g.v); }
function loop(): void { let s = 0; for (let i = 0; i < 4; i++) { const a = new A(Number(arg) + i); s += a.m(i); } console.log("loop", s); }
function mut(): void { const o = new M(Number(arg)); const r = o.inc(bump()); console.log("mut", r, o.f); }
function readAfter(): void { const a0 = new A(Number(arg) + 1); const r = a0.m(3); a0.v = r; console.log("readAfter", a0.v, a0.m(1)); }
plainConst(); plainBump(); sub(); acc(); loop(); mut(); readAfter();

// `o.f++` / `--o.f` on class fields: the class-field read/store route must
// keep `[[Get]]`, ToNumeric, step, `[[Set]]` semantics for every value kind.
class K {
  n = 1;
  s: any = "41";
  b: any = 10n;
  o: any = { valueOf: () => 7 };
  u: any = undefined;
  i = 2147483647;
  f = 0.5;
  step(): number[] { const a = this.n++; const p = ++this.n; const d = this.n--; const q = --this.n; return [a, p, d, q, this.n]; }
  str(): any[] { const a = this.s++; return [a, typeof a, this.s, typeof this.s]; }
  big(): any[] { const a = this.b++; const p = ++this.b; return [String(a), String(p), typeof this.b]; }
  obj(): any[] { const a = this.o++; return [a, this.o]; }
  undef(): any[] { const a = this.u++; return [a, this.u]; }
  edge(): number[] { this.i++; this.f--; return [this.i, this.f]; }
}
class Acc {
  private _v = 0;
  log: string[] = [];
  get v(): number { this.log.push("get"); return this._v; }
  set v(x: number) { this.log.push("set " + x); this._v = x; }
}
class Sub extends K { bump(): number { this.n++; this.n++; return this.n; } }
const k = new K();
console.log(k.step(), k.str(), k.big(), k.obj(), k.undef(), k.edge());
const acc = new Acc();
acc.v++;
++acc.v;
console.log(acc.v, acc.log.join(","));
const sub = new Sub();
console.log(sub.bump());
const loc = new K();
for (let i = 0; i < 1000; i++) loc.n++;
console.log(loc.n);
class Counter { nodes = 0; fullmove = 1; tick(white: boolean): void { this.nodes++; if (!white) this.fullmove++; } }
const c = new Counter();
for (let i = 0; i < 100; i++) c.tick(i % 2 === 0);
console.log(c.nodes, c.fullmove);

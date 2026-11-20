// Updates on class fields: the shape-proven numeric slot path and every
// declined case (representation change, freeze, subclass, accessors).
class P {
  n = 0;
  label = "p";
}
class Q extends P {
  m = 1;
}
function bump(p: P): number {
  return p.n++;
}
function mix(p: P): number {
  p.n += 2.5;
  p.n -= 1;
  p.n *= 3;
  p.n /= 2;
  return ++p.n;
}
const p = new P();
let sum = 0;
for (let i = 0; i < 1000; i++) sum += bump(p);
console.log(sum, p.n, mix(p), p.n, --p.n, p.n--, p.n);
const q = new Q();
for (let i = 0; i < 5; i++) bump(q);
console.log(q.n, q.m, mix(q));
// Same site after the lane stops being a Number.
(p as any).n = "41";
console.log(bump(p), p.n, typeof p.n);
(p as any).n = 7n;
console.log(String(bump(p)), String(p.n));
(p as any).n = { valueOf() { return 3; } };
console.log(bump(p), p.n);
// Non-finite and signed-zero results.
const r = new P();
r.n = 1e308;
r.n *= 10;
console.log(r.n);
r.n = NaN;
r.n++;
r.n += 1;
console.log(Number.isNaN(r.n));
r.n = -0;
r.n *= 1;
console.log(Object.is(r.n, -0));
r.n += 0;
console.log(Object.is(r.n, 0));
// Frozen receivers: class code is strict, so the write throws and the
// field keeps its value.
class Freezer {
  static run(p: P): string {
    try {
      p.n++;
      return "no-throw " + p.n;
    } catch (e) {
      return (e instanceof TypeError ? "TypeError " : "other ") + p.n;
    }
  }
}
const f = new P();
f.n = 5;
console.log(Freezer.run(f));
Object.freeze(f);
console.log(Freezer.run(f));
// Accessors on a class: one get and one set per update, in order.
let trace = "";
class G {
  _v = 1;
  get v(): number {
    trace += "get;";
    return this._v;
  }
  set v(x: number) {
    trace += "set:" + x + ";";
    this._v = x;
  }
  inc(): number {
    return this.v++;
  }
}
const g = new G();
console.log(g.v++, trace);
trace = "";
console.log(++g.v, trace);
trace = "";
console.log((g.v += 10), trace);
trace = "";
console.log(g.inc(), g._v, trace);
// `this.x` updates inside methods, plus the arena shape.
class Arena {
  left: Int32Array;
  size = 0;
  constructor(n: number) {
    this.left = new Int32Array(n);
  }
  push(v: number): number {
    const i = this.size++;
    this.left[i] = v;
    this.size += 0;
    return i;
  }
}
const a = new Arena(16);
for (let i = 0; i < 10; i++) a.push(i * 3);
console.log(a.size, a.left[9], a.size--, --a.size);
// Declared-type receivers that are not class instances: the declared class
// only nominates the guarded site, the shape decides.
const lit: any = { n: 5, label: "lit" };
const other: any = { m: 1, n: 2 };
const prox: any = new Proxy({ n: 10 }, {
  get(t: any, k: any) { trace += "pget:" + String(k) + ";"; return t[k]; },
  set(t: any, k: any, v: any) { trace += "pset:" + String(k) + "=" + v + ";"; t[k] = v; return true; },
});
trace = "";
console.log(bump(lit), lit.n, bump(other), other.n, bump(prox), prox.n, trace);
const described = new P();
let hidden = 3;
Object.defineProperty(described, "n", {
  get() { trace += "dget;"; return hidden; },
  set(v: number) { trace += "dset:" + v + ";"; hidden = v; },
});
trace = "";
console.log(bump(described), mix(described), hidden, trace);

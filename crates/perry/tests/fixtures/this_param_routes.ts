// Every route by which a JS body that reads its dynamic `this` is entered.
// Each caller hands the body its receiver as the `this` PARAMETER, the only
// way a body learns it; a route that passes the wrong receiver changes the
// printed sums.
const N = process.argv.length > 99 ? 1 : 300;
const out: string[] = [];
let s = 0;

function P(this: any, k: number) { this.k = k; }
(P as any).prototype.m = function (x: number) { return this.k + x; };
Object.defineProperty((P as any).prototype, "kk", {
  get: function (this: any) { return this.k * 10; },
  configurable: true,
});
const p1: any = new (P as any)(1);
const p2: any = new (P as any)(2);

const fexpr = function (this: any, x: number) {
  return (this && typeof this.k === "number" ? this.k : -1000) + x;
};
const owner: any = { k: 7 };
owner.f = function (this: any, x: number) { return this.k * x; };

function topLevel(this: any, x: number) {
  return (this && typeof this.k === "number" ? this.k : -1000) + x;
}
owner.t = topLevel;

class C {
  k = 5;
  m(x: number) { return this.k + x; }
}
class D extends C {
  g(x: number) { const f = super.m; return f.call(this, x); }
}
const d = new D();

const bound = fexpr.bind(owner);
// A plain call made while a method is running binds `this` to undefined
// (this module is strict): the method's receiver must not leak into a
// callback the runtime calls without one.
let leaks = 0;
const sorter: any = {
  k: 3,
  run(this: any) {
    const a = [3, 1, 2];
    a.sort(function (this: any, x: number, y: number) {
      if (this !== undefined) leaks++;
      return x - y;
    });
    const r = a.reduce(function (this: any, acc: number, v: number) {
      if (this !== undefined) leaks++;
      return acc + v;
    }, 0);
    return a.join("") + this.k + r;
  },
};
// Async and generator bodies bind their receiver at entry too.
const og: any = {
  k: 9,
  async am(this: any, x: number) { await 0; return this.k + x; },
  async an(this: any, x: number) { return this.k - x; },   // no await: not state-machine lowered
  *gm(this: any, x: number) { yield this.k + x; yield this.k * x; },
};

for (let i = 0; i < N; i++) {
  const o = i & 1 ? p1 : p2;
  s += o.m(i);                                  // ES5 prototype method, method site
  s += o.kk;                                    // prototype getter
  s += owner.f(i);                              // own function-valued property
  s += owner.t(i);                              // top-level function as a method
  s += fexpr(i);                                // receiverless call of a function expression
  s += fexpr.call(owner, i);                    // call
  s += fexpr.apply(p1, [i]);                    // apply
  s += bound(i);                                // bound function
  s += d.g(i);                                  // super method value (class method wrapper)
  [1, 2].forEach(function (this: any, v: number) { s += this.k * v; }, p2);  // callback + thisArg
  s += [4].map(function (this: any, v: number) { return this.k + v; }, owner)[0];
  const q: any = new (fexpr as any)(1);         // `new F()`
  s += q instanceof (fexpr as any) ? 1 : 0;
  const it = og.gm(i);                          // generator method
  s += it.next().value + it.next().value;
  if (i % 97 === 0) out.push(String(s));
}
out.push(sorter.run());
console.log(s, out.join(","), "leaks=" + leaks);
// Function EXPRESSIONS (not literal methods) installed as methods.
og.af = async function (this: any, x: number) { return this.k * x; };
og.gf = function* (this: any) { yield this.k; };
og.am(1).then((v: number) =>
  og.an(v).then((w: number) =>
    og.af(2).then((z: number) => console.log("async", v, w, z, og.gf().next().value))));

// Value wrappers of every function kind (installed as function objects).
export function exportedValue(this: any) { return typeof this; }
async function asyncValue(this: any) { return 1; }
function* genValue(this: any) { yield 1; }
const values: any[] = [exportedValue, asyncValue, genValue, topLevel];
console.log(values.map((f) => typeof f).join(","));

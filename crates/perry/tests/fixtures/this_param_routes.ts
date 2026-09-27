// Every route by which a JS body that reads `this` from the implicit-`this`
// cell is entered. Stage 1 of this-as-a-parameter: each caller must hand the
// body, as its `this` PARAMETER, exactly the receiver the cell holds.
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
const sorter: any = {
  k: 3,
  run(this: any) {
    const a = [3, 1, 2];
    a.sort(function (x: number, y: number) { return x - y; });
    return a.join("") + this.k;
  },
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
  if (i % 97 === 0) out.push(String(s));
}
out.push(sorter.run());
console.log(s, out.join(","));

// Value wrappers of every function kind (installed as function objects).
export function exportedValue(this: any) { return typeof this; }
async function asyncValue(this: any) { return 1; }
function* genValue(this: any) { yield 1; }
const values: any[] = [exportedValue, asyncValue, genValue, topLevel];
console.log(values.map((f) => typeof f).join(","));

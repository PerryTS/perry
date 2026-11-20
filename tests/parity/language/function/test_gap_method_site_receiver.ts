// Object-literal and prototype methods called through a method site receive
// their receiver as `this`: nested method calls, arrows inheriting `this`, a
// throw through a method, extra/missing arguments, `arguments`, the generic
// path (call/apply/detached), an inherited method, a getter, a function
// expression called and constructed, generator/async methods, and a strict
// function given a primitive. Output must match node.
const N = process.argv.length > 99 ? 1 : 3000;
function plainFn(this: any) { return this === globalThis; }
function mk(k: number): any {
  return {
    k,
    m(x: number) { return x + this.k; },
    outer(x: number) { const r = this.inner(x); return r + this.k; },
    inner(x: number) { return x * 2 + this.k; },
    thrower(x: number) { if (x < 0) throw new Error("neg" + this.k); return this.k; },
    wrap(x: number) { try { this.thrower(-x); } catch (e) { return this.k * 100; } return this.k; },
    arrow(x: number) { const f = () => this.k + x; return f(); },
    plain(x: number) { return plainFn() ? x + this.k : -1; },
    many(a: number, b: number, c: number, d: number) { return this.k + (b === undefined ? 0 : b) + (d === undefined ? 7 : d); },
    args(x: number) { return arguments.length + this.k; },
  };
}
// Generator and async methods live on their own literal.
function mkg(k: number): any {
  return { k, *gen() { yield this.k; yield this.k + 1; }, async am() { await null; return this.k * 3; } };
}
const a = mk(1);
const b = mk(2);
let s = 0, t = 0, v = 0, w = 0, y = 0, z = 0, q = 0, caught = 0;
for (let i = 0; i < N; i++) {
  const o = (i & 1) ? a : b;
  s += o.m(i);
  t += o.outer(i);
  v += o.arrow(i);
  w += o.plain(i);
  y += o.wrap(i + 1);
  z += o.many(i, 1);
  q += o.args(i, i);
  try { o.thrower(i % 7 === 0 ? -1 : i); } catch (e) { caught++; }
}
console.log(s, t, v, w, y, z, q, caught);
// The generic path (no site): call/apply/detached, through the public body.
const f = a.m;
console.log(a.m.call(b, 10), a.m.apply({ k: 40 }, [2]), f.call({ k: 7 }, 1), a.outer.call(b, 3));
// An inherited method through a site.
const proto: any = { m(x: number) { return x * 3 + this.k; } };
const kids: any[] = [];
for (let j = 0; j < 4; j++) { const c = Object.create(proto); c.k = j; kids.push(c); }
let r = 0;
for (let i = 0; i < N; i++) r += kids[i & 3].m(i);
console.log(r);
// A getter, and a function expression stored as a method, called and constructed.
const g: any = { k: 3, get dbl() { return this.k * 2; } };
let gs = 0;
for (let i = 0; i < N; i++) gs += g.dbl;
const holder: any = { k: 9 };
holder.F = function (this: any, x: number) { this.x = x; return this; };
let fs = 0;
for (let i = 0; i < N; i++) fs += holder.F(i).k;
const made = new holder.F(4);
console.log(gs, fs, holder.x, made.x, made === holder, made instanceof holder.F);
// Generator and async methods keep their receiver.
const ga = mkg(1);
const gb = mkg(2);
console.log(JSON.stringify([...ga.gen()]), JSON.stringify([...gb.gen()]));
ga.am().then((x: number) => console.log("async", x));

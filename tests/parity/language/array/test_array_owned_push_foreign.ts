// A push from another body (a closure, a method, a default) must not take
// the owned-array path of the body that declares the array.
const top: number[] = [];
function addTop(v: number) { top.push(v); }
for (let i = 0; i < 40; i++) addTop(i);
const frozen: number[] = [];
const cbs: Array<() => void> = [];
for (let i = 0; i < 3; i++) cbs.push(() => frozen.push(i));
cbs[0]();
Object.freeze(frozen);
try { cbs[1](); } catch (e) { console.log("frozen push throws", e instanceof TypeError); }
console.log(top.length, top[39], frozen.length);

function outer(n: number): number {
  const xs: number[] = [];
  const add = (v: number) => xs.push(v);
  for (let i = 0; i < n; i++) { xs.push(i); add(i * 2); }
  return xs.length + (xs[n] ?? 0);
}
console.log(outer(30));

function withDefault(n: number): number {
  const ys: number[] = [];
  const f = (a: number[] = ys) => a;
  for (let i = 0; i < n; i++) ys.push(i);
  const alias = f();
  alias.push(99);
  for (let i = 0; i < n; i++) ys.push(i);
  return ys.length + alias.length;
}
console.log(withDefault(20));

class Holder { items = top; }
const h = new Holder();
for (let i = 0; i < 20; i++) top.push(i);
console.log(h.items.length, h.items === top);

// fn.bind / fn.call / fn.apply answered from a method-site entry for the
// inherited %Function.prototype% intrinsic: a patched slot, an own shadowing
// property and an accessor redefinition are each seen at once.
const out: string[] = [];
function target(this: any, a?: any, b?: any) { return (this && this.tag) + ":" + a + ":" + b; }
function other(this: any) { return "other:" + (this && this.tag); }
const recv = { tag: "R" };
function site(f: any, mode: number): string {
  if (mode === 0) return f.bind(recv, 1)(2);
  if (mode === 1) return f.call(recv, 3, 4);
  return f.apply(recv, [5, 6]);
}
for (let i = 0; i < 6; i++) out.push(site(i % 2 ? target : other, i % 3));
// a patched Function.prototype.call is seen at once
const origCall = Function.prototype.call;
(Function.prototype as any).call = function (this: any) { return "patched-call"; };
out.push(site(target, 1));
(Function.prototype as any).call = origCall;
out.push(site(target, 1));
// an own property shadows the intrinsic
const shadowed: any = function (this: any) { return "x"; };
shadowed.bind = () => "own-bind";
out.push(String(site(shadowed, 1)));
try { out.push(String((shadowed as any).bind())); } catch (e) { out.push("threw"); }
// redefining bind as an accessor on the prototype
const origBind = Function.prototype.bind;
Object.defineProperty(Function.prototype, "bind", { get() { return function () { return () => "accessor-bind"; }; }, configurable: true });
out.push(String(site(target, 0)));
Object.defineProperty(Function.prototype, "bind", { value: origBind, writable: true, configurable: true });
out.push(String(site(target, 0)));
// bound functions and class methods as receivers, under GC pressure
class C { v = 7; m(this: any, k: number) { return this.v + k; } }
let sum = 0;
for (let i = 0; i < 20000; i++) {
  const c = new C();
  const bm = c.m.bind(c);
  sum += bm(i) + c.m.call(c, 1) + c.m.apply(c, [2]);
  const junk = [{ a: i }, { b: [i] }];
  sum += junk.length;
}
out.push("sum " + sum);
console.log(out.join("\n"));

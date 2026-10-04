// A method site that has built a Function.prototype intrinsic entry
// (fn.call / fn.bind / fn.apply on a function inheriting them) must see a
// later change of the slot at once, through the same site. The prototype and
// the keys are reached by names built at run time, so the compiler cannot see
// the change and every site keeps its method-site dispatch: only the entry's
// own compares stand between a changed slot and the intrinsic it was built
// for. Each slot is swapped for ANOTHER intrinsic, an own property shadows one
// on a single receiver, and a key is redefined as an accessor.
const G: any = globalThis;
const FP: any = G[["Func", "tion"].join("")].prototype;
const CALL = ["ca", "ll"].join(""), BIND = ["bi", "nd"].join(""), APPLY = ["app", "ly"].join("");
function target(this: any, a?: any, b?: any) { return (this && this.tag) + ":" + a + (b === undefined ? "" : ":" + b); }
const recv = { tag: "R" };
const holder: any = { f: target };
function show(r: any): string { return typeof r === "function" ? "fn(" + r() + ")" : String(r); }
function viaParam(f: any, i: number) { return f.call(recv, i); }
function sites(i: number): string {
  const b = holder.f.bind(recv, i);
  return [
    show(holder.f.call(recv, i)),
    show(b),
    show(holder.f.apply(recv, [i, "x"])),
    show(viaParam(target, i)),
  ].join(" ");
}
const out: string[] = [];
let acc = 0;
for (let i = 0; i < 100; i++) acc += sites(i).length;
out.push("warm " + acc);
out.push(sites(1));
const orig: any = { [CALL]: FP[CALL], [BIND]: FP[BIND], [APPLY]: FP[APPLY] };
// call := bind, apply := call, then restored
Reflect.set(FP, CALL, orig[BIND]);
out.push("call=bind: " + sites(2));
Reflect.set(FP, CALL, orig[CALL]);
out.push("call restored: " + sites(3));
Reflect.set(FP, APPLY, orig[CALL]);
out.push("apply=call: " + sites(4));
Reflect.set(FP, APPLY, orig[APPLY]);
out.push("apply restored: " + sites(5));
// an own property shadows the inherited intrinsic on one receiver
Reflect.set(holder.f, CALL, orig[APPLY]);
out.push("own call=apply: " + show(holder.f.call(recv, [6, "y"])));
Reflect.deleteProperty(holder.f, CALL);
out.push("own deleted: " + sites(7));
// the key redefined as an accessor that answers another intrinsic
Reflect.defineProperty(FP, CALL, { get() { return orig[BIND]; }, configurable: true });
out.push("accessor call=bind: " + sites(8));
Reflect.defineProperty(FP, CALL, { value: orig[CALL], writable: true, configurable: true });
out.push("accessor restored: " + sites(9));
console.log(out.join("\n"));

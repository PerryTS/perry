// globalThis is an ordinary object whose keys are atoms: a read of a name it
// does not own proves absence and continues to %Object.prototype%, while every
// way a program adds, deletes or replaces a global is seen by the next read.
const g: any = globalThis;
const out: string[] = [];
function absent(name: string): string {
  return typeof g[name] + "," + (name in g) + "," + Object.prototype.hasOwnProperty.call(g, name);
}
// typeof an absent global: identifier, member and computed reads
// @ts-ignore
out.push(typeof notAGlobalAnywhere);
out.push(typeof g.notAGlobalEither);
for (let i = 0; i < 3; i++) out.push(absent("nx" + "pa" + i));
// a miss on globalThis continues the walk to Object.prototype
(Object.prototype as any).nxpaInherited = 7;
out.push(String(g.nxpaInherited) + "," + Object.prototype.hasOwnProperty.call(g, "nxpaInherited"));
delete (Object.prototype as any).nxpaInherited;
out.push(String(g.nxpaInherited));
// a global added by a computed key whose text is built at run time
const dyn = ["nx", "pa", "Dyn"].join("");
g[dyn] = 41;
out.push(String(g.nxpaDyn) + "," + absent(dyn));
// @ts-ignore
out.push(String(typeof nxpaDyn === "number" ? nxpaDyn + 1 : "missing"));
// delete globalThis.x, then a read of it, then re-adding it
out.push(String(delete g[dyn]) + "," + absent(dyn) + "," + String(g.nxpaDyn));
g.nxpaDyn = "back";
out.push(String(g.nxpaDyn));
delete g.nxpaDyn;
out.push(typeof g.nxpaDyn);
// var-style global through defineProperty, then a non-configurable one
Object.defineProperty(g, "nxpaFixed", { value: 1, writable: false, configurable: false });
let deleted: string;
try {
  deleted = String(delete g.nxpaFixed);
} catch (e) {
  deleted = (e as Error).constructor.name;
}
out.push(deleted + "," + g.nxpaFixed);
// reassigning a builtin global is seen by the identifier and by globalThis
const OrigPromise = Promise;
const fakePromise: any = function FakePromise() {};
g.Promise = fakePromise;
out.push(String(Promise === fakePromise) + "," + String(g.Promise === fakePromise));
g.Promise = OrigPromise;
out.push(String(Promise === OrigPromise) + "," + typeof Promise.resolve);
const OrigArray = Array;
delete g.Array;
out.push(typeof g.Array + "," + ("Array" in g));
g.Array = OrigArray;
out.push(String(Array.isArray([])) + "," + (g.Array === OrigArray));
// builtin globals are own, non-enumerable data properties
for (const n of ["Array", "Math", "parseInt", "undefined", "NaN", "globalThis", "Symbol"]) {
  const d = Object.getOwnPropertyDescriptor(g, n)!;
  out.push(n + ":" + typeof d.value + "," + d.writable + "," + d.enumerable + "," + d.configurable);
}
console.log(out.join("\n"));

// PLAN 3h: Function.prototype reached by value (getPrototypeOf, a helper's
// parameter) and written there: the bind sites must see the write.
class C {
  constructor() { this.m = this.m.bind(this); }
  m(): string { return "m"; }
}
interface C { m: any; }
function f() { return 1; }
function install(proto: any, key: string, v: any) { proto[key] = v; }
const original = Function.prototype.bind;
const out: string[] = [];
for (let i = 0; i < 4; i++) {
  if (i === 2) { try { install(Object.getPrototypeOf(f), ["bi", "nd"].join(""), function () { return "indirect"; }); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
  const b = f.bind(null), m = new C().m;
  out.push([i, typeof b === "function" ? "builtin" : b, typeof m === "function" ? "builtin" : m].join(" "));
}
try { install(Object.getPrototypeOf(f), ["bi", "nd"].join(""), original); } catch (e) { console.log("DEC 250: " + (e as Error).message); }
for (const l of out) console.log(l);

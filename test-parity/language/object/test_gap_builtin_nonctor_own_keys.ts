// Built-in getters and non-constructor builtins have no own `prototype`:
// Reflect.ownKeys, getOwnPropertyNames and getOwnPropertyDescriptor must agree.
const L: any = (Intl as any).Locale.prototype;
const names = ["baseName","language","script","region","calendar","caseFirst","collation","hourCycle","numeric","firstDayOfWeek","numberingSystem"];
const fns: [string, any][] = [];
fns.push(["Array[@@species]", (Object.getOwnPropertyDescriptor(Array, Symbol.species) as any).get]);
for (const n of names) { const d = Object.getOwnPropertyDescriptor(L, n); fns.push(["Locale." + n, d && d.get]); }
fns.push(["Math.max", Math.max]);
fns.push(["Object.keys", Object.keys]);
fns.push(["Array.isArray", Array.isArray]);
fns.push(["Array.prototype.push", Array.prototype.push]);
fns.push(["JSON.parse", JSON.parse]);
fns.push(["parseInt", parseInt]);
fns.push(["Symbol", Symbol]);
fns.push(["Map[@@species]", (Object.getOwnPropertyDescriptor(Map, Symbol.species) as any).get]);
fns.push(["Object.prototype.__proto__", (Object.getOwnPropertyDescriptor(Object.prototype, "__proto__") as any).get]);
fns.push(["Promise.resolve", Promise.resolve]);
fns.push(["bound", function f() {}.bind(null)]);
fns.push(["arrow", () => 1]);
fns.push(["method", ({ m() {} }).m]);
fns.push(["async", async function a() {}]);
fns.push(["asyncarrow", async () => 1]);
fns.push(["function", function g() {}]);
fns.push(["class", class C {}]);
fns.push(["genfn", function* gg() {}]);
fns.push(["Array", Array]);
fns.push(["Object", Object]);
fns.push(["Date", Date]);
fns.push(["Reflect.ownKeys", Reflect.ownKeys]);
for (const [n, f] of fns) {
  if (typeof f !== "function") { console.log(n, "NOT FUNCTION", typeof f); continue; }
  console.log(n, Reflect.ownKeys(f).includes("prototype"), Object.getOwnPropertyDescriptor(f, "prototype") !== undefined, Object.getOwnPropertyNames(f).includes("prototype"));
}

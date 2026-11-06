// %Function.prototype%: its own keys, attributes and inherited members (ECMA-262 §20.2.3, §10.2.4).
export {};
const FP: any = Function.prototype;
const OP: any = Object.prototype;
console.log(Object.getOwnPropertyNames(FP).join(","));
console.log(Reflect.ownKeys(FP).map((k) => String(k)).join(","));
for (const k of ["length", "name", "constructor", "apply", "bind", "call", "toString"]) {
  console.log(k, JSON.stringify(Object.getOwnPropertyDescriptor(FP, k), (_, v) => (typeof v === "function" ? "fn:" + v.name : v)));
}
for (const k of ["caller", "arguments"]) {
  const d: any = Object.getOwnPropertyDescriptor(FP, k);
  console.log(k, typeof d.get, d.get === d.set, d.enumerable, d.configurable, "value" in d);
}
console.log(["valueOf", "toLocaleString", "isPrototypeOf", "hasOwnProperty", "propertyIsEnumerable"].map((k) => k + ":" + FP.hasOwnProperty(k) + ":" + (FP[k] === OP[k])).join(" "));
console.log(Object.getPrototypeOf(FP) === OP, typeof FP, FP.length, JSON.stringify(FP.name));
function f() {}
console.log(f.hasOwnProperty("valueOf"), f.hasOwnProperty("toString"), f.hasOwnProperty("caller"), "valueOf" in f, "name" in f, "caller" in f, "call" in f);
for (const k of ["caller", "arguments"]) {
  try { (f as any)[k]; console.log(k, "read"); } catch (e: any) { console.log(k, "throws", e instanceof TypeError); }
}

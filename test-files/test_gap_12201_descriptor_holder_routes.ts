// Every JS property-holder representation must reach owned descriptor storage.
// GC capture boxes, weak storage and opaque FFI payloads are internal cells;
// boxed JS primitives, WeakMap objects and native registry handles are holders.
class Holder { x = 1; }
function fn() { return 1; }
const target: any = {};
const holders: any[] = [
  {}, [], fn, Holder, new Holder(), new Map(), new Set(), Promise.resolve(1),
  new Error("holder"), new Date(0), Temporal.PlainDate.from("2026-10-08"),
  new Headers(), new Proxy(target, {}), new Number(3), new Boolean(true),
  new String("abc"), Object(1n), Object(Symbol("boxed")), /x/g,
  new WeakMap(), new WeakSet(), new ArrayBuffer(8), new Uint8Array(0),
  new DataView(new ArrayBuffer(8))
];
for (let i = 0; i < holders.length; i++) {
  const holder: any = holders[i];
  Object.defineProperty(holder, "lane12201", {
    value: i + 10, writable: true, enumerable: true, configurable: true
  });
  const before: any = Object.getOwnPropertyDescriptor(holder, "lane12201");
  console.log(i, holder.lane12201, before.value, before.writable, before.enumerable, before.configurable);
  // Restrict the property through defineProperty on every representation;
  // some native APIs have their own integrity-level contracts.
  Object.defineProperty(holder, "lane12201", { value: i + 10, writable: false, configurable: false });
  const after: any = Object.getOwnPropertyDescriptor(holder, "lane12201");
  console.log("restricted", i, after.value, after.writable, after.enumerable, after.configurable);
}
console.log("proxy target", Object.getOwnPropertyDescriptor(target, "lane12201")!.writable);
// A primitive is never a holder. Symbols are pointer-tagged, so the Object
// test every define consults must still say no and throw, not drop the define.
const primitives: any[] = [Symbol("p"), Symbol.iterator, "str", 7n, 1.5, true];
for (const p of primitives) {
  const kind = typeof p;
  for (const [name, op] of [
    ["defineProperty", () => Object.defineProperty(p, "lane12201", { value: 1 })],
    ["defineProperties", () => Object.defineProperties(p, { lane12201: { value: 1 } })],
    ["Reflect.defineProperty", () => Reflect.defineProperty(p, "lane12201", { value: 1 })],
    ["create", () => Object.create(p)],
    ["setPrototypeOf", () => Object.setPrototypeOf({}, p)],
  ] as [string, () => unknown][]) {
    try { op(); console.log(kind, name, "no throw"); }
    catch (e: any) { console.log(kind, name, e.constructor.name); }
  }
  console.log(kind, "gopd", Object.getOwnPropertyDescriptor(p, "lane12201"),
    "gopds", JSON.stringify(Object.getOwnPropertyDescriptors(p)).length > 1, "then", typeof p.then);
  console.log(kind, "integrity", Object.isFrozen(p), Object.isSealed(p), Object.isExtensible(p),
    Object.freeze(p) === p, Object.seal(p) === p, Object.preventExtensions(p) === p,
    Object.isFrozen(p), Object.isSealed(p), Object.isExtensible(p));
}
// Promise combinators probe `then` on every element, primitives included.
Promise.all([Promise.resolve(1), "plain", Symbol.iterator, 3n]).then((v) => console.log("all", v.length, String(v[1])));
Promise.allSettled(["a", Symbol("q")]).then((v) => console.log("allSettled", v.length, v[1].status));
Promise.race(["r", 2]).then((v) => console.log("race", v));
Promise.any([Symbol("y"), "z"]).then((v) => console.log("any", typeof v));

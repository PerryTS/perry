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

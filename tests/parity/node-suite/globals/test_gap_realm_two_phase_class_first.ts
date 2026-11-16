// Two-phase realm: the first touch of the realm is a class used as a value
// (the core). The phase-B families are then reached without naming their
// globals first: instances made by `new`, inherited reads, `constructor`.
class Base { static made = 0; constructor() { if (new.target === Base) Base.made++; } }
class Derived extends Base { static label = "derived"; }
const classes = [Base, Derived];
console.log("classes", classes.map((c) => c.name).join(","), new Derived() instanceof Base, Base.made);

const key = { k: 1 };
const wm = new WeakMap<object, number>();
wm.set(key, 7);
const anyWm: any = wm;
console.log("weakmap dynamic", anyWm["get"](key), anyWm["has"]({}), typeof anyWm.delete);
console.log("weakmap tag", Object.prototype.toString.call(wm), String(wm));
console.log("weakmap proto", Object.getPrototypeOf(wm) === WeakMap.prototype, Object.getPrototypeOf(wm).constructor === WeakMap);
const own = (o: object, wanted: string[]) => wanted.filter((n) => Object.getOwnPropertyNames(o).includes(n)).join(",");
console.log("weakmap names", own(WeakMap.prototype, ["constructor", "delete", "get", "has", "set", "add"]));

const ws = new WeakSet<object>([key]);
console.log("weakset", ws.has(key), (ws as any).constructor.name, Object.getPrototypeOf(ws) === WeakSet.prototype);
console.log("weakset names", own(WeakSet.prototype, ["constructor", "delete", "get", "has", "set", "add"]));

const wr = new WeakRef(key);
console.log("weakref", wr.deref() === key, (wr as any).constructor === WeakRef, Object.prototype.toString.call(wr));
console.log("weakref names", Object.getOwnPropertyNames(WeakRef.prototype).join(","));

const fr = new FinalizationRegistry(() => {});
console.log("registry", typeof (fr as any).register, (fr as any).constructor.name, Object.prototype.toString.call(fr));

const target = { a: 1 };
const proxy = new Proxy(target, { get: (t: any, p) => (p === "a" ? 41 + t.a : undefined) });
console.log("proxy", (proxy as any).a, typeof Proxy, Proxy.length, typeof Proxy.revocable);

console.log("namespaces", typeof Atomics, typeof Intl, typeof WebAssembly, Object.prototype.toString.call(Atomics));
console.log("atomics", Atomics.add(new Int32Array(new SharedArrayBuffer(8)), 0, 5), typeof Atomics.wait);
console.log("intl", new Intl.NumberFormat("en-US").format(1234.5), typeof Intl.DateTimeFormat);
console.log("wasm", typeof WebAssembly.Module, typeof WebAssembly.instantiate);
console.log("still core", Base.made, typeof Map, typeof Promise, [1, 2, 3].map((x) => x * 2).join(","));

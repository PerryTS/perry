// `o[i]` and `o[i] = v` on an object that is no Array: a Date, a RegExp, a
// Map, a Promise, an Error, a function, a boxed primitive, an ArrayBuffer, a
// weak collection, a class instance, an arguments object, a Proxy. Such an
// object has no element vector, so the index is an ordinary property key:
// absent on a fresh object (never a word of the object's own memory), own
// after a store or a defineProperty, an accessor's result, an inherited
// value. The same through an `any` receiver, an array annotation that the
// value does not honour (`number[]`, `any[]`, `string[]`), a
// `Record<number, T>` and at module scope. Negative controls: an Array, a
// typed array, a String object and an arguments object keep their elements.
export {};

function show(label: string, f: () => any): void {
  let r: string;
  try {
    const v = f();
    r = typeof v + ":" + String(v);
  } catch (e: any) {
    r = "throw " + (e && e.constructor && e.constructor.name);
  }
  console.log(label + " -> " + r);
}

class K {
  x = 1;
}

const MK: [string, () => any][] = [
  ["Date", () => new Date(0)],
  ["RegExp", () => /a/g],
  ["Map", () => new Map()],
  ["Set", () => new Set()],
  ["Promise", () => Promise.resolve(1)],
  ["Error", () => new Error("x")],
  ["TypeError", () => new TypeError("x")],
  ["AggregateError", () => new AggregateError([], "m")],
  ["Function", () => function () {}],
  ["Arrow", () => () => 1],
  ["bound", () => function () {}.bind(null)],
  ["Number", () => new Number(5)],
  ["Boolean", () => new Boolean(true)],
  ["Symbol object", () => Object(Symbol("s"))],
  ["BigInt object", () => Object(10n)],
  ["ArrayBuffer", () => new ArrayBuffer(8)],
  ["DataView", () => new DataView(new ArrayBuffer(8))],
  ["WeakMap", () => new WeakMap()],
  ["WeakSet", () => new WeakSet()],
  ["class instance", () => new K()],
  ["Proxy", () => new Proxy({}, {})],
  ["object literal", () => ({})],
  ["null prototype", () => Object.create(null)],
  ["generator", () => (function* () { yield 1; })()],
  // Negative controls: these do have elements.
  ["Array", () => [10, 20]],
  ["Uint8Array", () => new Uint8Array(2)],
  ["Float64Array", () => new Float64Array(3)],
  ["String object", () => new String("abc")],
  ["arguments", () => (function (..._a: any[]) { return arguments; })(7, 8)],
];

function rdAnyKey(o: any, i: number): any { return o[i]; }
function rdAny1(o: any): any { return o[1]; }
function rdNum(a: number[], i: number): any { return a[i]; }
function rdAnyArr(a: any[], i: number): any { return a[i]; }
function rdStr(a: string[], i: number): any { return a[i]; }
function rdConst(a: number[]): any { return a[1]; }
function rdRec(o: Record<number, any>, i: number): any { return o[i]; }
function loop(a: number[]): string { let s = ""; for (let i = 0; i < 4; i++) s += String(a[i]) + ","; return s; }
function loopAny(o: any): string { let s = ""; for (let i = 0; i < 4; i++) s += String(o[i]) + ","; return s; }
function wrAnyKey(o: any, i: number, v: any): any { o[i] = v; return o[i]; }
function wrNum(a: number[], i: number, v: number): any { a[i] = v; return a[i]; }
function wrAnyArr(a: any[], i: number, v: any): any { a[i] = v; return a[i]; }
function wrRec(o: Record<number, any>, i: number, v: any): any { o[i] = v; return o[i]; }
function wrLoop(a: number[]): string { for (let i = 0; i < 3; i++) a[i] = i + 0.5; return String(a[0]) + "," + String(a[2]); }
function upd(a: number[], i: number): any { a[i] += 2; a[i]++; return a[i]; }
function own(o: any, i: number): string { return (i in o) + "/" + Object.prototype.hasOwnProperty.call(o, i); }

// Left out, because they are other defects (wrong, not unsafe): an index
// inherited through a replaced prototype of a Promise, an ArrayBuffer or a
// DataView, and a key that is no array index (-1, 1.5) on an ArrayBuffer or
// a DataView.
const LEFT_OUT: { [test: string]: string[] } = {
  inherited: ["Promise", "ArrayBuffer", "DataView"],
  "other keys": ["ArrayBuffer", "DataView"],
};
function tested(name: string, test: string): boolean {
  return !(LEFT_OUT[test] || []).includes(name);
}

for (const [name, make] of MK) {
  // A fresh object: nothing at any index, near or far.
  show(name + " fresh any", () => { const o = make(); return [o[0], o[1], o[5], o[100000], rdAnyKey(o, 0), rdAnyKey(o, 1), rdAnyKey(o, 7), rdAnyKey(o, 123456), rdAnyKey(o, 2147483647), rdAny1(o)].map(String).join("|"); });
  show(name + " fresh number[]", () => { const o = make(); return [rdNum(o, 0), rdNum(o, 1), rdNum(o, 7), rdNum(o, 123456), rdConst(o)].map(String).join("|") + " " + loop(o); });
  show(name + " fresh any[] string[] Record", () => { const o = make(); return [rdAnyArr(o, 0), rdAnyArr(o, 1), rdStr(o, 0), rdStr(o, 1), rdRec(o, 0), rdRec(o, 3)].map(String).join("|") + " " + loopAny(o); });
  // A store creates an own property that every reader finds.
  show(name + " store any", () => { const o = make(); const w = wrAnyKey(o, 1, "A"); return [w, o[1], rdAnyKey(o, 1), rdNum(o, 1), rdAnyArr(o, 1), rdRec(o, 1), o["1"], o[0], rdAnyKey(o, 5), own(o, 1)].map(String).join("|"); });
  show(name + " store number[]", () => { const o = make(); const w = wrNum(o, 1, 4.5); return [w, rdNum(o, 1), rdAnyKey(o, 1), rdAnyKey(o, 0), own(o, 1)].map(String).join("|"); });
  show(name + " store any[]", () => { const o = make(); const w = wrAnyArr(o, 2, "s"); return [w, rdAnyArr(o, 2), rdAnyKey(o, 2), own(o, 2)].map(String).join("|"); });
  show(name + " store Record", () => { const o = make(); return [rdRec(o, 3), wrRec(o, 3, "q"), rdRec(o, 3), rdAnyKey(o, 3)].map(String).join("|"); });
  show(name + " store loop", () => { const o = make(); return wrLoop(o) + "|" + loop(o) + "|" + own(o, 1); });
  show(name + " store far", () => { const o = make(); return [wrNum(o, 100000, 1.5), rdNum(o, 100000), rdNum(o, 99999), rdAnyKey(o, 100000)].map(String).join("|"); });
  show(name + " update", () => { const o = make(); wrNum(o, 1, 1); return String(upd(o, 1)) + "|" + String(rdAnyKey(o, 1)); });
  // defineProperty: a data property, a getter, a setter, a read-only value.
  show(name + " define value", () => { const o = make(); Object.defineProperty(o, 1, { value: "D", writable: true, enumerable: true, configurable: true }); return [o[1], rdAnyKey(o, 1), rdNum(o, 1), rdAnyArr(o, 1), rdConst(o), o[0], rdAnyKey(o, 5)].map(String).join("|") + " " + loop(o); });
  show(name + " define getter", () => { const o = make(); Object.defineProperty(o, 2, { get() { return "G"; }, configurable: true }); return [o[2], rdAnyKey(o, 2), rdNum(o, 2), rdAnyArr(o, 2), o[1], o[3]].map(String).join("|"); });
  show(name + " define setter", () => { const o = make(); let got = "none"; Object.defineProperty(o, 1, { set(v: any) { got = String(v); }, configurable: true }); wrAnyKey(o, 1, 9.5); const a = got; wrNum(o, 1, 8.5); return a + "|" + got + "|" + String(rdAnyKey(o, 1)); });
  show(name + " define read-only", () => { const o = make(); Object.defineProperty(o, 0, { value: "R", writable: false, configurable: true }); try { wrAnyKey(o, 0, "X"); } catch (e) {} try { wrNum(o, 0, 1); } catch (e) {} return String(o[0]) + "|" + String(rdNum(o, 0)); });
  // An inherited index.
  if (tested(name, "inherited")) show(name + " inherited", () => { const o = make(); const p = Object.create(Object.getPrototypeOf(o)); p[3] = "P"; Object.setPrototypeOf(o, p); return [o[3], rdAnyKey(o, 3), rdNum(o, 3), rdAnyArr(o, 3), o[2], rdAnyKey(o, 4)].map(String).join("|"); });
  // Keys that are no array index.
  if (tested(name, "other keys")) show(name + " other keys", () => { const o = make(); wrAnyKey(o, -1, "n"); wrAnyKey(o, 1.5, "f"); wrAnyKey(o, 4294967295, "B"); return [rdAnyKey(o, -1), rdAnyKey(o, 1.5), rdAnyKey(o, 4294967295), rdAnyKey(o, 4294967294), o["-1"], o["1.5"]].map(String).join("|"); });
  show(name + " string key", () => { const o = make(); o["1"] = "S"; return [rdAnyKey(o, 1), rdNum(o, 1), o[1], o["1"]].map(String).join("|"); });
  show(name + " delete", () => { const o = make(); wrAnyKey(o, 1, 3); let d = "?"; try { d = String(delete o[1]); } catch (e) { d = "threw"; } return d + "|" + String(rdAnyKey(o, 1)) + "|" + String(rdNum(o, 1)); });
  show(name + " frozen", () => { const o = make(); Object.freeze(o); try { wrAnyKey(o, 1, 3); } catch (e) {} try { wrNum(o, 1, 3); } catch (e) {} return String(rdAnyKey(o, 1)) + "|" + String(rdNum(o, 1)); });
}

// Module scope, with and without an annotation the value does not honour.
const d1 = new Date(0) as any as number[];
console.log("module Date", String(d1[0]), String(d1[1]), String(d1[5]));
d1[1] = 3.5;
console.log("module Date after store", String(d1[1]), String(d1[0]), Object.keys(d1).join(","));
const e1: any = new Error("x");
console.log("module Error", String(e1[0]), String(e1[1]));
e1[0] = "z";
console.log("module Error after store", String(e1[0]), String(e1.message), Object.keys(e1).join(","));
const p1: any = Promise.resolve(42);
console.log("module Promise", String(p1[0]), String(p1[1]));
p1[0] = "z";
console.log("module Promise after store", String(p1[0]), Object.keys(p1).join(","));
p1.then((v: any) => console.log("the promise keeps its value", v));
// The value an object holds is not reachable by index.
const d2: any = new Date(1234567890123);
let leak = 0;
for (let i = 0; i < 64; i++) if (d2[i] !== undefined) leak++;
console.log("Date indices that read something", leak, d2.getTime());
const p2: any = Promise.resolve("secret");
let leak2 = 0;
for (let i = 0; i < 64; i++) if (p2[i] !== undefined) leak2++;
console.log("Promise indices that read something", leak2);

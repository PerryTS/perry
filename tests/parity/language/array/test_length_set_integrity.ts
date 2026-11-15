"use strict";
// `arr.length = n` is ArraySetLength, whatever the Array's integrity level:
// a non-extensible Array shrinks (it loses the elements that can be deleted)
// and grows (a longer length adds no property); a sealed Array keeps every
// element, so a shorter length is rejected at its highest element; a frozen
// Array and a read-only `length` reject every change; a non-configurable
// element stops the truncation above it. A rejection is silent in sloppy
// code, a TypeError in strict code and `false` from `Reflect.set`.
// A typed array's `length`, `byteLength`, `byteOffset` and `buffer` are
// accessors of %TypedArray%.prototype without a setter: an assignment is
// rejected the same way and creates no own property.
// This file: strict code.
function show(label: string, f: () => any): void {
  let r: string;
  try { r = String(f()); } catch (e: any) { r = "throw " + (e && e.constructor && e.constructor.name); }
  console.log(label + " -> " + r);
}
function d(a: any): string { return "len=" + a.length + " [" + Array.prototype.join.call(a, ",") + "] keys=" + Object.keys(a).join("|") + " ext=" + Object.isExtensible(a) + " sealed=" + Object.isSealed(a) + " frozen=" + Object.isFrozen(a); }
const STATES: [string, (a: any) => any][] = [
  ["plain", (a) => a],
  ["preventExtensions", (a) => Object.preventExtensions(a)],
  ["sealed", (a) => Object.seal(a)],
  ["frozen", (a) => Object.freeze(a)],
  ["length read-only", (a) => Object.defineProperty(a, "length", { writable: false })],
  ["index 1 non-configurable", (a) => Object.defineProperty(a, 1, { configurable: false })],
  ["index 1 non-configurable + preventExtensions", (a) => Object.preventExtensions(Object.defineProperty(a, 1, { configurable: false }))],
  ["index 0 getter", (a) => Object.defineProperty(a, 0, { get() { return "g"; }, configurable: true })],
];
function setLenAny(a: any, n: any): void { a.length = n; }
function setLenArr(a: number[], n: number): void { a.length = n; }
for (const [name, prep] of STATES) {
  for (const n of [2, 0, 3, 5, 1]) {
    show(name + " any.length=" + n, () => { const a: any = prep([1, 2, 3]); let t = "ok"; try { setLenAny(a, n); } catch (e: any) { t = "threw " + e.constructor.name; } return t + " " + d(a); });
    show(name + " number[].length=" + n, () => { const a: number[] = prep([1, 2, 3]); let t = "ok"; try { setLenArr(a, n); } catch (e: any) { t = "threw " + e.constructor.name; } return t + " " + d(a); });
    show(name + " literal.length=" + n, () => { const a = prep([1, 2, 3]); let t = "ok"; try { a.length = n; } catch (e: any) { t = "threw " + e.constructor.name; } return t + " " + d(a); });
  }
  show(name + " Reflect.set length 1", () => { const a: any = prep([1, 2, 3]); return Reflect.set(a, "length", 1) + " " + d(a); });
  show(name + " defineProperty length 1", () => { const a: any = prep([1, 2, 3]); let t = "ok"; try { Object.defineProperty(a, "length", { value: 1 }); } catch (e: any) { t = "threw " + e.constructor.name; } return t + " " + d(a); });
}
// typed arrays: `length` is an accessor without a setter on %TypedArray%.prototype
const TA: [string, () => any][] = [["Uint8Array", () => new Uint8Array(3)], ["Float64Array", () => new Float64Array(3)], ["Int32Array", () => new Int32Array(3)], ["BigInt64Array", () => new BigInt64Array(3)], ["Buffer", () => Buffer.alloc(3)], ["Uint8ClampedArray", () => new Uint8ClampedArray(3)]];
function taSet(t: any, n: any): void { t.length = n; }
function taSetTyped(t: Uint8Array, n: number): void { (t as any).length = n; }
for (const [name, mk] of TA) {
  show(name + " length=1", () => { const t = mk(); let r = "ok"; try { taSet(t, 1); } catch (e: any) { r = "threw " + e.constructor.name; } return r + " len=" + t.length + " own=" + Object.prototype.hasOwnProperty.call(t, "length") + " keys=" + Object.keys(t).join("|") + " names=" + Object.getOwnPropertyNames(t).join("|"); });
  show(name + " length=10", () => { const t = mk(); let r = "ok"; try { taSet(t, 10); } catch (e: any) { r = "threw " + e.constructor.name; } return r + " len=" + t.length + " own=" + Object.prototype.hasOwnProperty.call(t, "length") + " [" + Array.prototype.join.call(t, ",") + "]"; });
  show(name + " literal length", () => { const t = mk(); let r = "ok"; try { t.length = 2; } catch (e: any) { r = "threw " + e.constructor.name; } return r + " len=" + t.length + " own=" + Object.prototype.hasOwnProperty.call(t, "length"); });
  show(name + " typed param", () => { const t = mk(); let r = "ok"; try { taSetTyped(t, 2); } catch (e: any) { r = "threw " + e.constructor.name; } return r + " len=" + t.length + " own=" + Object.prototype.hasOwnProperty.call(t, "length"); });
  show(name + " Reflect.set", () => { const t = mk(); return Reflect.set(t, "length", 1) + " len=" + t.length + " own=" + Object.prototype.hasOwnProperty.call(t, "length"); });
  show(name + " byteLength/byteOffset/buffer", () => { const t = mk(); let r = ""; for (const k of ["byteLength", "byteOffset", "buffer"]) { try { t[k] = 1; r += "ok,"; } catch (e: any) { r += "T,"; } } return r + " " + t.byteLength + " " + t.byteOffset + " own=" + Object.getOwnPropertyNames(t).filter((k: string) => isNaN(Number(k))).join("|"); });
  show(name + " defineProperty length", () => { const t = mk(); let r = "ok"; try { Object.defineProperty(t, "length", { value: 1, configurable: true }); } catch (e: any) { r = "threw " + e.constructor.name; } return r + " len=" + t.length + " own=" + Object.prototype.hasOwnProperty.call(t, "length"); });
  show(name + " subsequent ops", () => { const t = mk(); try { taSet(t, 1); } catch (e) {} t[2] = 5; return t.length + " " + Array.prototype.join.call(t, ",") + " " + Array.from(t.slice(1)).join(","); });
}

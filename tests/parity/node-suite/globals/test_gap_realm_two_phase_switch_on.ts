// parity-env: PERRY_REALM_TWO_PHASE=1
// Two-phase realm with the switch set explicitly (the header above; it is
// also the default): the lanes that link and run on each platform exercise
// the strong link-time hooks of the entry module. Its twin with the switch
// off is test_gap_realm_two_phase_switch_off.ts. The first touch is a class
// value (the core alone); then phase-B families are reached through
// instances, by name and by a store.
class Core { static n = 1; }
const first = [Core];
console.log("core", first[0].n, typeof Map, [3, 1, 2].sort().join(","));
const key = { k: 1 };
const wm = new WeakMap<object, number>([[key, 7]]);
const anyWm: any = wm;
console.log("weakmap", anyWm["get"](key), Object.getPrototypeOf(wm) === WeakMap.prototype);
console.log("weakref", new WeakRef(key).deref() === key, typeof FinalizationRegistry);
const proxy: any = new Proxy({ a: 1 }, { get: (t: any, p) => (p === "a" ? 41 + t.a : undefined) });
console.log("proxy", proxy.a, typeof Proxy.revocable);
const bytes = new Uint8Array([1, 2, 3]);
console.log("bytes", bytes.length, Object.getPrototypeOf(bytes) === Uint8Array.prototype, bytes.join("-"));
const d = new Date(Date.UTC(2020, 0, 2, 3, 4, 5));
console.log("date", d.toISOString(), Object.getPrototypeOf(d) === Date.prototype);
console.log("names", typeof Atomics, typeof Intl, typeof WebAssembly, typeof globalThis.WeakSet);
const names = Object.getOwnPropertyNames(globalThis);
console.log("order", names.indexOf("Array") < names.indexOf("WeakMap"), names.includes("Intl"));
(globalThis as any).WeakRef = "mine";
console.log("store", (globalThis as any).WeakRef, typeof WeakMap, first[0].n);

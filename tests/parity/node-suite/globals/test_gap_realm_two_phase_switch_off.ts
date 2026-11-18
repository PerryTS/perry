// parity-env: PERRY_REALM_TWO_PHASE=0
// The two-phase realm switched OFF (the header above): the one case of this
// family every runner compiles for a realm populated in one pass, so the
// lanes that link and run on each platform exercise the runtime's weak
// link-time hooks (the program defines none of them). The same program as
// test_gap_realm_two_phase_switch_on.ts: the first touch is a class value,
// then the families phase B binds are reached through instances, by name and
// by a store.
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

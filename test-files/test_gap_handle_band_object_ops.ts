// #6271 follow-up — generic object ops on POINTER_TAG *registry handles*.
//
// zlib streams, fetch Request/Response/Headers/Blob, crypto hashes and revocable
// Proxies are small integer handles NaN-boxed under POINTER_TAG, not heap
// addresses (see perry-runtime/src/value/addr_class.rs). Runtime paths that
// dereference the payload without a handle-band check read unmapped low memory
// and SIGSEGV — on Linux only; macOS masks the class behind its 2 TB heap floor.
// 44 of these (receiver, op) pairs crashed before the fix.
//
// This asserts the ops COMPLETE and agree with Node. It deliberately does not
// assert own-key *counts* for the native handles: Node's Gzip/Hash/Headers are
// ordinary JS objects carrying internal own fields (_readableState, …) while
// Perry models them as opaque native handles with none. That difference is
// long-standing and by design; the crash is the bug.
import * as zlib from "node:zlib";
import * as crypto from "node:crypto";

const makers: Array<[string, () => any]> = [
  ["gzipStream", () => zlib.createGzip({ level: 1 })],
  ["headers", () => new Headers({ "x-a": "1" })],
  ["request", () => new Request("https://example.com")],
  ["response", () => new Response("hi")],
  ["blob", () => new Blob(["hi"])],
  ["hash", () => crypto.createHash("sha256")],
];

// Ops whose result is representation-independent: they must not crash, and
// must return the same answer Node gives for an absent property.
const ops: Array<[string, (x: any) => unknown]> = [
  ["delete absent", (x) => delete x.__nope],
  ["hasOwnProperty absent", (x) => Object.prototype.hasOwnProperty.call(x, "__nope")],
  ["propertyIsEnumerable absent", (x) => Object.prototype.propertyIsEnumerable.call(x, "__nope")],
  ["getOwnPropertyDescriptor absent", (x) => Object.getOwnPropertyDescriptor(x, "__nope") === undefined],
  ["in absent", (x) => "__nope" in x],
  ["keys is array", (x) => Array.isArray(Object.keys(x))],
  ["values is array", (x) => Array.isArray(Object.values(x))],
  ["entries is array", (x) => Array.isArray(Object.entries(x))],
  ["getOwnPropertyNames is array", (x) => Array.isArray(Object.getOwnPropertyNames(x))],
  ["spread is object", (x) => typeof { ...x } === "object"],
  ["for-in completes", (x) => { let n = 0; for (const _k in x) n++; return typeof n === "number"; }],
  ["defineProperty completes", (x) => { Object.defineProperty(x, "__d", { value: 1 }); return true; }],
  ["freeze returns receiver", (x) => Object.freeze(x) === x],
];

for (const [name, make] of makers) {
  for (const [opName, op] of ops) {
    const x = make();
    try {
      console.log(`${name}.${opName}: ${op(x)}`);
    } catch (e: any) {
      console.log(`${name}.${opName}: throw ${e && e.constructor && e.constructor.name}`);
    }
  }
}

// A revocable Proxy is ALSO a handle-band id, but unlike the native handles it
// must still reflect its target's own properties through the ownKeys/get traps.
// Guarding the band without routing proxies to those traps would silently
// report an empty object — so pin the actual values here.
const p: any = new Proxy({ a: 1, b: 2 }, {});
console.log(`proxy.keys: ${JSON.stringify(Object.keys(p))}`);
console.log(`proxy.values: ${JSON.stringify(Object.values(p))}`);
console.log(`proxy.entries: ${JSON.stringify(Object.entries(p))}`);
console.log(`proxy.hasOwnProperty a: ${Object.prototype.hasOwnProperty.call(p, "a")}`);
console.log(`proxy.delete a: ${delete p.a}`);
console.log(`proxy.keys after delete: ${JSON.stringify(Object.keys(p))}`);
console.log("done");

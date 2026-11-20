// Test missing global APIs: structuredClone, queueMicrotask, performance, AbortController, atob/btoa, WeakRef, FinalizationRegistry

// === structuredClone ===

// Basic deep clone
const original = { a: 1, b: [2, 3] };
const cloned = structuredClone(original);
console.log(cloned.a); // 1
console.log(cloned.b[0]); // 2
console.log(cloned.b[1]); // 3

// Verify it's a deep copy (mutating clone doesn't affect original)
cloned.b.push(4);
console.log(original.b.length); // 2
console.log(cloned.b.length); // 3

// Nested objects
const nested = { x: { y: { z: 42 } } };
const nestedClone = structuredClone(nested);
console.log(nestedClone.x.y.z); // 42
nestedClone.x.y.z = 99;
console.log(nested.x.y.z); // 42

// Clone with Date
const withDate = { d: new Date("2025-01-15T00:00:00Z") };
const dateClone = structuredClone(withDate);
console.log(dateClone.d instanceof Date); // true
console.log(dateClone.d.toISOString()); // 2025-01-15T00:00:00.000Z

// Clone with RegExp
const withRegex = { r: /hello/gi };
const regexClone = structuredClone(withRegex);
console.log(regexClone.r instanceof RegExp); // true
console.log(regexClone.r.source); // hello
console.log(regexClone.r.flags); // gi

// Clone with Map
const withMap = { m: new Map([["key1", "val1"], ["key2", "val2"]]) };
const mapClone = structuredClone(withMap);
console.log(mapClone.m instanceof Map); // true
console.log(mapClone.m.get("key1")); // val1
console.log(mapClone.m.size); // 2
mapClone.m.set("key3", "val3");
console.log(withMap.m.size); // 2

// Clone with Set
const withSet = { s: new Set([1, 2, 3]) };
const setClone = structuredClone(withSet);
console.log(setClone.s instanceof Set); // true
console.log(setClone.s.has(2)); // true
console.log(setClone.s.size); // 3
setClone.s.add(4);
console.log(withSet.s.size); // 3

// Clone with arrays of mixed types
const mixed = [1, "hello", true, null, undefined, { a: 1 }];
const mixedClone = structuredClone(mixed);
console.log(mixedClone.length); // 6
console.log(mixedClone[0]); // 1
console.log(mixedClone[1]); // hello
console.log(mixedClone[2]); // true
console.log(mixedClone[3]); // null
console.log(mixedClone[4]); // undefined
console.log(mixedClone[5].a); // 1

// === queueMicrotask ===
let microtaskRan = false;
queueMicrotask(() => {
  microtaskRan = true;
});
// Microtask runs after current synchronous code but before next macrotask
// We check it after awaiting a resolved promise (which also uses microtask queue)
await Promise.resolve();
console.log("microtaskRan:", microtaskRan); // microtaskRan: true

// Multiple microtasks execute in order
const order: string[] = [];
queueMicrotask(() => order.push("first"));
queueMicrotask(() => order.push("second"));
queueMicrotask(() => order.push("third"));
await Promise.resolve();
// Need another tick to ensure all are flushed
await new Promise<void>(resolve => queueMicrotask(resolve));
console.log("microtask order:", order.join(",")); // microtask order: first,second,third

// === performance.now() ===
const t1 = performance.now();
console.log("performance.now type:", typeof t1); // performance.now type: number
console.log("performance.now > 0:", t1 > 0); // performance.now > 0: true

// Monotonicity: second call should be >= first
const t2 = performance.now();
console.log("monotonic:", t2 >= t1); // monotonic: true

// Time passes during work
let sum = 0;
for (let i = 0; i < 1000000; i++) {
  sum += i;
}
const t3 = performance.now();
console.log("elapsed > 0:", t3 > t1); // elapsed > 0: true

// === AbortController ===
const controller = new AbortController();
const signal = controller.signal;
console.log("signal.aborted before:", signal.aborted); // signal.aborted before: false

controller.abort();
console.log("signal.aborted after:", signal.aborted); // signal.aborted after: true

// Abort with reason
const controller2 = new AbortController();
controller2.abort("custom reason");
console.log("abort reason:", controller2.signal.reason); // abort reason: custom reason

// Signal event listener
const controller3 = new AbortController();
let abortHandled = false;
controller3.signal.addEventListener("abort", () => {
  abortHandled = true;
});
controller3.abort();
console.log("abort event handled:", abortHandled); // abort event handled: true

// AbortSignal.timeout
const timeoutSignal = AbortSignal.timeout(100);
console.log("timeout signal aborted initially:", timeoutSignal.aborted); // timeout signal aborted initially: false

// === atob / btoa ===
const encoded = btoa("hello");
console.log("btoa:", encoded); // btoa: aGVsbG8=

const decoded = atob("aGVsbG8=");
console.log("atob:", decoded); // atob: hello

// Round-trip
const roundTrip = atob(btoa("Hello, World!"));
console.log("round-trip:", roundTrip); // round-trip: Hello, World!

// Binary data
const binaryEncoded = btoa(String.fromCharCode(0, 1, 2, 255));
const binaryDecoded = atob(binaryEncoded);
console.log("binary length:", binaryDecoded.length); // binary length: 4
console.log("binary byte 0:", binaryDecoded.charCodeAt(0)); // binary byte 0: 0
console.log("binary byte 3:", binaryDecoded.charCodeAt(3)); // binary byte 3: 255

// === WeakRef ===
let target: { value: number } | undefined = { value: 42 };
const weakRef = new WeakRef(target);
console.log("weakRef.deref():", weakRef.deref()?.value); // weakRef.deref(): 42

// WeakRef with different object types
const arrTarget = [1, 2, 3];
const weakArr = new WeakRef(arrTarget);
console.log("weakArr deref length:", weakArr.deref()?.length); // weakArr deref length: 3

// === FinalizationRegistry ===
let registryCallbackCalled = false;
const registry = new FinalizationRegistry((heldValue: string) => {
  registryCallbackCalled = true;
});

// Register an object (we can't force GC, so just verify registration doesn't crash)
let tempObj: object | null = { data: "test" };
registry.register(tempObj, "cleanup-token");
console.log("registry created:", true); // registry created: true

// Unregister
const unregToken = {};
let tempObj2: object | null = { data: "test2" };
registry.register(tempObj2, "token2", unregToken);
const unregistered = registry.unregister(unregToken);
console.log("unregistered:", unregistered); // unregistered: true

console.log("All global API tests passed!");

// Expected output:
// 1
// 2
// 3
// 2
// 3
// 42
// 42
// true
// 2025-01-15T00:00:00.000Z
// true
// hello
// gi
// true
// val1
// 2
// 2
// true
// true
// 3
// 3
// 6
// 1
// hello
// true
// null
// undefined
// 1
// microtaskRan: true
// microtask order: first,second,third
// performance.now type: number
// performance.now > 0: true
// monotonic: true
// elapsed > 0: true
// signal.aborted before: false
// signal.aborted after: true
// abort reason: custom reason
// abort event handled: true
// timeout signal aborted initially: false
// btoa: aGVsbG8=
// atob: hello
// round-trip: Hello, World!
// binary length: 4
// binary byte 0: 0
// binary byte 3: 255
// weakRef.deref(): 42
// weakArr deref length: 3
// registry created: true
// unregistered: true
// All global API tests passed!

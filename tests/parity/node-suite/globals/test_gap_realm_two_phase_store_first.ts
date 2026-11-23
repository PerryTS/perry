// Two-phase realm: a program store to a declared phase-B name before
// anything read it. The store must win: completing the realm afterwards
// must not put the built-in back.
class Marker { static tag = "m"; }
const holder = [Marker];
console.log("core", holder[0].tag);
// @ts-ignore: assignment to a global built-in
Proxy = 1;
console.log("Proxy", Proxy);
var Intl = 2;
console.log("Intl", Intl);
(globalThis as any).WeakRef = "mine";
console.log("WeakRef", (globalThis as any).WeakRef, typeof WeakMap, typeof Atomics);
// @ts-ignore: assignment to a global built-in
WebAssembly = { replaced: true };
console.log("WebAssembly", JSON.stringify(WebAssembly));
console.log("after", Proxy, Intl, (globalThis as any).Proxy, typeof (globalThis as any).FinalizationRegistry);

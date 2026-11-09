// `performance.now` can be replaced by a harness or polyfill, from any
// module, and every later call must run the replacement: the bare global
// call, a call through an alias, and a call through `globalThis`.
const real = performance.now();
console.log(typeof real === "number" && real >= 0);

const fake = () => 42;
Object.defineProperty(globalThis.performance, "now", { value: fake, configurable: true, writable: true });
const alias = performance;
console.log(performance.now(), alias.now(), globalThis.performance.now(), performance.now === fake);

performance.now = () => 7;
console.log(performance.now(), alias.now());

// Restoring the original brings the native clock back.
delete (performance as any).now;
const restored = performance.now();
console.log(typeof restored === "number" && restored !== 7 && restored !== 42);

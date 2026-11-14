// Two-phase realm: a getter on the global object's prototype chain is reached
// by the read of a name the realm does not declare, with the global object as
// `this`. It must see the complete global, whatever the realm had built before
// (the first touch here is a class value: the core alone).
//
// The getter sits on an object linked in front of Object.prototype:
// Object.prototype itself takes no new member in a compiled program (DEC 250),
// so this is the one way a program can put a getter there.
class Core { static n = 1; }
const first = [Core];
console.log("core", first[0].n);
const proto = Object.create(Object.prototype, {
  __realmProbe: {
    get() { return typeof (this as any).WeakMap + ":" + typeof (this as any).Intl + ":" + typeof (this as any).Uint8Array; },
    configurable: true,
  },
});
let linked = true;
try { Object.setPrototypeOf(globalThis, proto); } catch (e) { linked = false; }
console.log("linked", linked);
declare var __realmProbe: string;
console.log("typeof", typeof __realmProbe);
console.log("bare", __realmProbe);
console.log("member", (globalThis as any).__realmProbe, (globalThis as any)["__realm" + "Probe"]);
console.log("in", "__realmProbe" in globalThis, Object.hasOwn(globalThis, "__realmProbe"));
console.log("after", typeof WeakMap, first[0].n);

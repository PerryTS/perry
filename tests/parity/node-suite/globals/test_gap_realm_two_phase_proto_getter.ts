// Two-phase realm: a getter the program puts on Object.prototype is reached
// by the read of a name the realm does not declare, with the global object
// as `this`. It must see the complete global, whatever the realm had built
// before (the first touch here is a class value: the core alone).
class Core { static n = 1; }
const first = [Core];
console.log("core", first[0].n);
Object.defineProperty(Object.prototype, "__realmProbe", {
  get() { return typeof (this as any).WeakMap + ":" + typeof (this as any).Intl + ":" + typeof (this as any).Uint8Array; },
  configurable: true,
});
declare var __realmProbe: string;
console.log("bare", __realmProbe);
console.log("typeof", typeof __realmProbe);
console.log("member", (globalThis as any).__realmProbe, (globalThis as any)["__realm" + "Probe"]);
console.log("in", "__realmProbe" in globalThis, Object.hasOwn(globalThis, "__realmProbe"));
delete (Object.prototype as any).__realmProbe;
console.log("after", typeof __realmProbe, typeof WeakMap, first[0].n);

// Reflect.get(target, key, receiver): [[Get]] walks target's prototype chain,
// and when the resolved property is an accessor, its getter runs with `this`
// bound to the RECEIVER — not to whichever object in the chain owns the
// getter. Perry handled an own getter via a receiver, but an INHERITED getter
// fell back to a plain read that ignored the receiver, returning undefined.
// An own data property shadows an inherited accessor (no receiver effect).
// test262: built-ins/Reflect/get/return-value-from-receiver.js
function log(label: string, value: unknown): void {
  console.log(label, String(value));
}

const base: Record<string, unknown> = {};
Object.defineProperty(base, "x", {
  get(this: { y?: unknown }) {
    return this.y;
  },
  configurable: true,
});
const receiver = { y: 42 };

// Own accessor, distinct receiver → getter `this` is the receiver.
log("own", Reflect.get(base, "x", receiver));

// Inherited accessor (one and two links up) → still bound to the receiver.
const mid = Object.create(base) as Record<string, unknown>;
log("proto", Reflect.get(mid, "x", receiver));
log("deep", Reflect.get(Object.create(mid) as object, "x", receiver));

// Own data property shadows the inherited accessor → plain read, no receiver.
const shadow = Object.create(base) as Record<string, unknown>;
Object.defineProperty(shadow, "x", { value: "own", writable: true });
log("shadow", Reflect.get(shadow, "x", receiver));

// Inherited accessor with no getter → undefined.
const noGet: Record<string, unknown> = {};
Object.defineProperty(noGet, "z", { set() {}, configurable: true });
log("nogetter", Reflect.get(Object.create(noGet) as object, "z", receiver));

// Omitted receiver defaults to target.
const selfish: Record<string, unknown> = {};
Object.defineProperty(selfish, "w", {
  get(this: unknown) {
    return this === selfish ? "self" : "other";
  },
});
log("default-receiver", Reflect.get(selfish, "w"));

// Plain data reads (own + inherited) are unchanged.
log("own-data", Reflect.get({ a: 1 }, "a", receiver));
log("inherited-data", Reflect.get(Object.create({ k: 7 }) as object, "k", receiver));

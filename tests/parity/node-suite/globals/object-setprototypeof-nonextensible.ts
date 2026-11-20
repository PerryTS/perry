// OrdinarySetPrototypeOf: a non-extensible object rejects a *changing*
// [[Prototype]]. Object.setPrototypeOf surfaces that as a TypeError; a no-op
// set to the SAME prototype still succeeds, and extensible / primitive targets
// are unaffected. Reflect.setPrototypeOf returns `false` (never throws) on the
// same reject. test262: built-ins/Reflect/preventExtensions/prevent-extensions.js
function probe(label: string, run: () => unknown): void {
  try {
    run();
    console.log(label, "ok");
  } catch (e: unknown) {
    const ctor = e && (e as { constructor?: { name?: string } }).constructor;
    console.log(label, "threw", ctor ? ctor.name : typeof e);
  }
}

// Non-extensible target + changing prototype → TypeError.
const a: Record<string, unknown> = {};
Object.preventExtensions(a);
probe("preventExt change", () => Object.setPrototypeOf(a, Array.prototype));

// Non-extensible target + SAME prototype → no-op success (no throw).
const b: Record<string, unknown> = {};
const bProto = Object.getPrototypeOf(b);
Object.preventExtensions(b);
probe("preventExt same", () => Object.setPrototypeOf(b, bProto));

// Frozen target (also non-extensible) + changing prototype → TypeError.
const c = Object.freeze({ x: 1 });
probe("frozen change", () => Object.setPrototypeOf(c, Array.prototype));

// Extensible target → succeeds.
const d: Record<string, unknown> = {};
probe("extensible change", () => Object.setPrototypeOf(d, Array.prototype));
console.log("extensible applied", Object.getPrototypeOf(d) === Array.prototype);

// Primitive target → spec no-op, never throws.
probe("primitive", () => Object.setPrototypeOf(5 as unknown as object, Array.prototype));

// Reflect.setPrototypeOf returns false (no throw) on the same reject.
const e: Record<string, unknown> = {};
Object.preventExtensions(e);
console.log("reflect change", (Reflect as { setPrototypeOf(o: object, p: object | null): boolean }).setPrototypeOf(e, Array.prototype));
console.log("reflect same", (Reflect as { setPrototypeOf(o: object, p: object | null): boolean }).setPrototypeOf(e, Object.getPrototypeOf(e)));

// Perry monomorphizes a generic class: `new Gen<number>()` is stamped with a
// SEPARATE class id (`monomorph::mangle::generate_specialized_name` →
// `Gen$num`). TypeScript erases type arguments, so at runtime there is exactly
// one `Gen`, one `Gen.prototype` and one `Gen.prototype.constructor` — the
// specializations are an implementation detail that must not reach any
// id-keyed identity or lookup surface.
//
// Three faces of the same leak were fixed one at a time: `instanceof` (#7575),
// `constructor.name` (#7632), and the two prototype-object registries (#7762).
// This file pins the two that remained — the CONSTRUCTOR VALUE, and the
// PROPERTY LOOKUP chain, which is the one that made a prototype patch
// invisible on a specialized instance.
//
// #7632 made the constructor case worse before it got better: two constructors
// that print identically and compare unequal is a harder failure to debug than
// an obviously-wrong name.

class Gen<T> {
  v: T | undefined;
  tell(): string {
    return "gen";
  }
}

const a = new Gen<number>();
const b = new Gen<string>();
const c = new Gen(); // never specialized

// --- constructor identity ---------------------------------------------------
console.log("a===Gen:", a.constructor === Gen);
console.log("b===Gen:", b.constructor === Gen);
console.log("c===Gen:", c.constructor === Gen);
console.log("a===b:", a.constructor === b.constructor);
console.log("name:", (a.constructor as any).name);

// The prototype edge and the constructor edge must AGREE. They disagreed for a
// release: `getPrototypeOf(a) === Gen.prototype` was already true while
// `a.constructor === Gen` was false.
console.log("proto===Gen.prototype:", Object.getPrototypeOf(a) === Gen.prototype);
console.log("protoA===protoB:", Object.getPrototypeOf(a) === Object.getPrototypeOf(b));
console.log("a.ctor===proto.ctor:", a.constructor === (Object.getPrototypeOf(a) as any).constructor);

// --- prototype LOOKUP, not just prototype identity --------------------------
// A patch on `Gen.prototype` must be visible from a specialized instance. It
// was not: the instance's chain walk keyed on the specialization's id and never
// reached the generic's entries, so this read was `undefined` while
// `getPrototypeOf(a) === Gen.prototype` reported true.
(Gen.prototype as any).patched = "P";
(Gen.prototype as any).patchedMethod = function (this: any) {
  return "pm";
};
console.log("a.patched:", (a as any).patched, (b as any).patched, (c as any).patched);
console.log("a.patchedMethod():", (a as any).patchedMethod(), (b as any).patchedMethod());

// Declared methods still work — they dispatch off the per-class-id vtable,
// which is deliberately NOT aliased so each specialization keeps its own
// monomorphized bodies.
console.log("tell:", a.tell(), b.tell(), c.tell());

// --- the edge must not collapse everything into everything ------------------
class Other<T> {
  w: T | undefined;
}
const o = new Other<number>();
console.log("o===Other:", o.constructor === Other);
console.log("o!==Gen:", o.constructor !== (Gen as any));
console.log("a instanceof Gen:", a instanceof Gen, "| o instanceof Gen:", o instanceof Gen);

// A specialization of a SUBCLASS reports the subclass, not the base: the origin
// edge is an alias for one class, not a walk up the parent chain.
class Base {
  tag = "base";
}
class Sub<T> extends Base {
  u: T | undefined;
}
const s = new Sub<number>();
console.log("s===Sub:", s.constructor === Sub, "| s!==Base:", s.constructor !== (Base as any));
console.log("s instanceof Sub:", s instanceof Sub, "| s instanceof Base:", s instanceof Base);

// Constructing through the reported constructor round-trips.
console.log("new a.ctor instanceof Gen:", new (a.constructor as any)() instanceof Gen);

// `instanceof` must walk the REAL [[Prototype]] chain, not a synthetic class-id
// chain stamped at construction. The ES5 inheritance idiom
// `Derived.prototype = Object.create(Base.prototype)` installs the chain AFTER
// the constructor is defined, so the class-id shortcut never learned the
// `Derived -> Base` edge and `new Derived() instanceof Base` wrongly returned
// false. react-server-dom's flight Chunk inherits `Promise.prototype` exactly
// this way (`Chunk.prototype = Object.create(Promise.prototype)`), so this
// pattern shows up in real Next.js SSR.
//
// Perry already walks the real chain for method lookup + `getPrototypeOf`; only
// the `instanceof` operator took the id-based shortcut. The fix adds a spec
// `OrdinaryHasInstance` prototype-walk fallback.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

function Base(this: any) {}
(Base as any).prototype.hi = function () { return "hi"; };

// (1) two-level chain with the common `constructor` fix-up idiom
function Derived(this: any) {}
(Derived as any).prototype = Object.create((Base as any).prototype);
(Derived as any).prototype.constructor = Derived;
const d: any = new (Derived as any)();
console.log(d instanceof Derived, d instanceof Base, d.hi());

// (2) two-level chain WITHOUT the constructor fix-up (the flight Chunk shape)
function D2(this: any) {}
(D2 as any).prototype = Object.create((Base as any).prototype);
const d2: any = new (D2 as any)();
console.log(d2 instanceof D2, d2 instanceof Base);
console.log(Object.getPrototypeOf(d2) === (D2 as any).prototype);
console.log(Object.getPrototypeOf((D2 as any).prototype) === (Base as any).prototype);
console.log(d2.hi());

// (3) three-level chain: Grand -> Mid -> Leaf, all via reassigned prototypes
function Grand(this: any) {}
(Grand as any).prototype.g = function () { return "g"; };
function Mid(this: any) {}
(Mid as any).prototype = Object.create((Grand as any).prototype);
function Leaf(this: any) {}
(Leaf as any).prototype = Object.create((Mid as any).prototype);
const leaf: any = new (Leaf as any)();
console.log(leaf instanceof Leaf, leaf instanceof Mid, leaf instanceof Grand);
console.log(leaf.g());

// (4) unrelated types must still be false (no over-matching from the fallback)
function Other(this: any) {}
const o: any = new (Other as any)();
console.log(o instanceof Base, d instanceof Other, leaf instanceof Other);

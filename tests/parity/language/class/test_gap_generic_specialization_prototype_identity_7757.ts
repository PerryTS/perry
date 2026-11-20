// #7757: monomorphized specializations must share the GENERIC's prototype
// object. `new Gen<number>()` compiles to a second class `Gen$num` with its own
// class id, but TypeScript erases type arguments — at runtime there is exactly
// one `Gen` and one `Gen.prototype`.
//
// Before this fix each specialization materialized its own prototype, so
// `getPrototypeOf(a) === Gen.prototype` was true while
// `getPrototypeOf(a) === getPrototypeOf(b)` was false — the registry answered
// differently depending on which class id you asked through.
//
// NOT covered here: `a.constructor === Gen` (still divergent, tracked on
// #7757). The `.constructor` edge resolves through a different path than
// either prototype registry.

class Gen<T> {
  v: T | undefined;
}
class Wrap<T> extends Gen<T> {}

const a = new Gen<number>();
const b = new Gen<string>();
const c = new Gen(); // never specialized

console.log("proto a === Gen.prototype:", Object.getPrototypeOf(a) === Gen.prototype);
console.log("proto a === proto b:", Object.getPrototypeOf(a) === Object.getPrototypeOf(b));
console.log("proto a === proto c:", Object.getPrototypeOf(a) === Object.getPrototypeOf(c));

const w = new Wrap<number>();
console.log("nested proto === Wrap.prototype:", Object.getPrototypeOf(w) === Wrap.prototype);
console.log("nested chain:", Object.getPrototypeOf(Object.getPrototypeOf(w)) === Gen.prototype);

// The #7575 and #7632 halves keep working.
console.log("instanceof:", a instanceof Gen, b instanceof Gen, w instanceof Gen);
console.log("names:", a.constructor.name, b.constructor.name, Gen.name);

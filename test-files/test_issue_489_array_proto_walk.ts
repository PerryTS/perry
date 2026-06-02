// Refs #489 (drizzle + DB end-to-end): drizzle's `is(value, type)` walks
// the prototype chain via `cls = Object.getPrototypeOf(value).constructor`
// then loops `cls = Object.getPrototypeOf(cls)` until it reaches null. When
// `value` is a plain array (drizzle passes array chunks of column values
// through `is()` while building an INSERT), `cls` becomes the global
// `Array` constructor.
//
// Pre-fix, Perry's `Object.getPrototypeOf` returned the closure *itself*
// for a function/constructor receiver, so `Object.getPrototypeOf(Array)
// === Array` — an infinite self-cycle that hung the walk forever and made
// `db.insert(...).values([...]).run()` never return. Node terminates the
// walk at null (Array → Function.prototype → Object.prototype → null);
// Perry doesn't model those intermediate prototypes but now also
// terminates at null. The booleans below are identical across both
// runtimes; a regression flips them (and would hang an unbounded walk).

const arr = [1, 2, 3];
const ctor = Object.getPrototypeOf(arr).constructor; // the global Array

// The load-bearing invariant: a constructor must not be its own prototype.
console.log("array ctor self-cycle:", Object.getPrototypeOf(ctor) === ctor);

// A bounded walk up from the array constructor must terminate well under
// the cap. Pre-fix this spun until the cap (cls stayed === ctor forever).
let cls: any = ctor;
let steps = 0;
while (cls && steps < 50) {
    steps++;
    cls = Object.getPrototypeOf(cls);
}
console.log("array ctor walk terminated:", steps < 50);

// Same for a plain object literal's constructor.
const objCtor = Object.getPrototypeOf({ a: 1 }).constructor;
console.log("object ctor self-cycle:", objCtor ? Object.getPrototypeOf(objCtor) === objCtor : false);

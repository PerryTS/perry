// Gap test: Object methods not yet supported by Perry
// Run: node --experimental-strip-types test_gap_object_methods.ts

// --- Object.fromEntries from array of pairs ---
const entries1: [string, number][] = [["a", 1], ["b", 2], ["c", 3]];
const obj1 = Object.fromEntries(entries1);
console.log("fromEntries array:", obj1); // { a: 1, b: 2, c: 3 }
console.log("fromEntries a:", obj1.a); // 1
console.log("fromEntries b:", obj1.b); // 2

// --- Object.fromEntries from Map ---
const map = new Map<string, number>();
map.set("x", 10);
map.set("y", 20);
map.set("z", 30);
const obj2 = Object.fromEntries(map);
console.log("fromEntries map:", obj2); // { x: 10, y: 20, z: 30 }
console.log("fromEntries x:", obj2.x); // 10

// Round-trip: Object.entries -> Object.fromEntries
const orig = { foo: 1, bar: 2, baz: 3 };
const roundTrip = Object.fromEntries(Object.entries(orig));
console.log("round-trip:", roundTrip); // { foo: 1, bar: 2, baz: 3 }
console.log("round-trip foo:", roundTrip.foo); // 1

// --- Object.is ---
console.log("Object.is(NaN, NaN):", Object.is(NaN, NaN)); // true
console.log("Object.is(0, -0):", Object.is(0, -0)); // false
console.log("Object.is(0, 0):", Object.is(0, 0)); // true
console.log("Object.is(1, 1):", Object.is(1, 1)); // true
console.log("Object.is(1, 2):", Object.is(1, 2)); // false
console.log("Object.is(null, null):", Object.is(null, null)); // true
console.log("Object.is(undefined, undefined):", Object.is(undefined, undefined)); // true
console.log("Object.is(null, undefined):", Object.is(null, undefined)); // false
console.log("Object.is('a', 'a'):", Object.is("a", "a")); // true

// Contrast with === for edge cases
console.log("NaN === NaN:", NaN === NaN); // false (unlike Object.is)
console.log("0 === -0:", 0 === -0); // true (unlike Object.is)

// --- Object.hasOwn ---
const hasOwnObj = { name: "Perry", version: 1 };
console.log("hasOwn name:", Object.hasOwn(hasOwnObj, "name")); // true
console.log("hasOwn version:", Object.hasOwn(hasOwnObj, "version")); // true
console.log("hasOwn missing:", Object.hasOwn(hasOwnObj, "missing")); // false
console.log("hasOwn toString:", Object.hasOwn(hasOwnObj, "toString")); // false (inherited)

// With null prototype
const nullProto = Object.create(null);
nullProto.key = "value";
console.log("hasOwn null proto:", Object.hasOwn(nullProto, "key")); // true
console.log("hasOwn null proto missing:", Object.hasOwn(nullProto, "other")); // false

// #3527: Object.hasOwn as a first-class VALUE (not a direct call). The
// feature-detect idiom in iconv-lite's merge-exports stores it in a var via a
// ternary, which defeats the const-alias call-fold, so the value itself must be
// a real callable. typeof must agree, and the bound value must compute hasOwn.
console.log("hasOwn typeof value:", typeof Object.hasOwn); // function
const hasOwnVal = typeof Object.hasOwn === "undefined" ? null : Object.hasOwn;
console.log("hasOwn via var:", hasOwnVal!(hasOwnObj, "name"), hasOwnVal!(hasOwnObj, "missing")); // true false
const hasOwnArr: Array<(o: object, k: string) => boolean> = [Object.hasOwn];
console.log("hasOwn via array:", hasOwnArr[0](hasOwnObj, "version")); // true
const ReboundO: any = Object;
console.log("hasOwn rebound:", ReboundO.hasOwn(hasOwnObj, "name")); // true

// --- Object.defineProperty ---
const defObj: Record<string, unknown> = {};
Object.defineProperty(defObj, "readOnly", {
  value: 42,
  writable: false,
  enumerable: true,
  configurable: false,
});
console.log("defineProperty value:", defObj.readOnly); // 42

// Attempt to write (should silently fail in non-strict, throw in strict)
try {
  (defObj as any).readOnly = 100;
} catch {
  console.log("defineProperty write threw (strict mode)");
}
console.log("defineProperty still 42:", defObj.readOnly); // 42

// Non-enumerable property
Object.defineProperty(defObj, "hidden", {
  value: "secret",
  writable: true,
  enumerable: false,
  configurable: true,
});
console.log("hidden value:", defObj.hidden); // 'secret'
console.log("hidden in keys:", Object.keys(defObj).includes("hidden")); // false

// --- Object.getOwnPropertyDescriptor ---
const descObj = { visible: 1 };
Object.defineProperty(descObj, "custom", {
  value: 99,
  writable: false,
  enumerable: false,
  configurable: true,
});

const desc1 = Object.getOwnPropertyDescriptor(descObj, "visible");
console.log("descriptor visible:", desc1); // { value: 1, writable: true, enumerable: true, configurable: true }
console.log("descriptor visible writable:", desc1?.writable); // true
console.log("descriptor visible enumerable:", desc1?.enumerable); // true

const desc2 = Object.getOwnPropertyDescriptor(descObj, "custom");
console.log("descriptor custom writable:", desc2?.writable); // false
console.log("descriptor custom enumerable:", desc2?.enumerable); // false
console.log("descriptor custom configurable:", desc2?.configurable); // true

const descMissing = Object.getOwnPropertyDescriptor(descObj, "nope");
console.log("descriptor missing:", descMissing); // undefined

// --- Object.getOwnPropertyNames ---
const namesObj: Record<string, unknown> = { a: 1, b: 2 };
Object.defineProperty(namesObj, "hidden", {
  value: 3,
  enumerable: false,
});
const allNames = Object.getOwnPropertyNames(namesObj);
console.log("getOwnPropertyNames:", allNames); // ['a', 'b', 'hidden']
console.log("includes hidden:", allNames.includes("hidden")); // true
console.log("Object.keys excludes hidden:", Object.keys(namesObj).includes("hidden")); // false

// --- Object.getPrototypeOf ---
class Animal {
  speak() { return "..."; }
}
class Dog extends Animal {
  speak() { return "woof"; }
}
const dog = new Dog();
console.log("getPrototypeOf dog === Dog.prototype:", Object.getPrototypeOf(dog) === Dog.prototype); // true
console.log("getPrototypeOf Dog.prototype === Animal.prototype:", Object.getPrototypeOf(Dog.prototype) === Animal.prototype); // true

const plainObj = { x: 1 };
console.log("getPrototypeOf plain === Object.prototype:", Object.getPrototypeOf(plainObj) === Object.prototype); // true

// --- Object.isFrozen, Object.isSealed, Object.isExtensible ---
const extObj = { a: 1 };
console.log("isExtensible:", Object.isExtensible(extObj)); // true
console.log("isFrozen:", Object.isFrozen(extObj)); // false
console.log("isSealed:", Object.isSealed(extObj)); // false

// Freeze
const frozenObj: Record<string, number> = { x: 1, y: 2 };
Object.freeze(frozenObj);
console.log("after freeze isExtensible:", Object.isExtensible(frozenObj)); // false
console.log("after freeze isFrozen:", Object.isFrozen(frozenObj)); // true
console.log("after freeze isSealed:", Object.isSealed(frozenObj)); // true

// Freeze actually prevents mutations
try {
  (frozenObj as any).x = 999;
} catch {
  console.log("freeze write threw");
}
console.log("frozen x still 1:", frozenObj.x); // 1

try {
  (frozenObj as any).newProp = "nope";
} catch {
  console.log("freeze add threw");
}
console.log("frozen newProp:", (frozenObj as any).newProp); // undefined

// Seal (writable but not extensible/configurable)
const sealedObj: Record<string, number> = { a: 1, b: 2 };
Object.seal(sealedObj);
console.log("after seal isSealed:", Object.isSealed(sealedObj)); // true
console.log("after seal isExtensible:", Object.isExtensible(sealedObj)); // false
console.log("after seal isFrozen:", Object.isFrozen(sealedObj)); // false (writable)

// Sealed: can modify existing, cannot add new
sealedObj.a = 100;
console.log("sealed modify a:", sealedObj.a); // 100
try {
  (sealedObj as any).c = 3;
} catch {
  console.log("sealed add threw");
}
console.log("sealed c:", (sealedObj as any).c); // undefined

// preventExtensions
const prevExtObj: Record<string, number> = { p: 1 };
Object.preventExtensions(prevExtObj);
console.log("preventExtensions isExtensible:", Object.isExtensible(prevExtObj)); // false
prevExtObj.p = 999;
console.log("preventExtensions modify p:", prevExtObj.p); // 999 (still writable)
try {
  (prevExtObj as any).q = 2;
} catch {
  console.log("preventExtensions add threw");
}
console.log("preventExtensions q:", (prevExtObj as any).q); // undefined

// --- Object.getOwnPropertySymbols ---
const sym1 = Symbol("first");
const sym2 = Symbol("second");
const symObj: Record<string | symbol, unknown> = { normal: "yes" };
(symObj as any)[sym1] = "one";
(symObj as any)[sym2] = "two";

const symbols = Object.getOwnPropertySymbols(symObj);
console.log("getOwnPropertySymbols count:", symbols.length); // 2
console.log("symbols includes sym1:", symbols.includes(sym1)); // true
console.log("symbols includes sym2:", symbols.includes(sym2)); // true
console.log("Object.keys excludes symbols:", Object.keys(symObj).length); // 1 (only 'normal')

// --- Getter / setter property descriptors ---
const accessorObj: Record<string, unknown> = {};
let _backing = 0;
Object.defineProperty(accessorObj, "computed", {
  get() { return _backing * 2; },
  set(v: number) { _backing = v; },
  enumerable: true,
  configurable: true,
});

accessorObj.computed = 5;
console.log("getter result:", accessorObj.computed); // 10
console.log("backing value:", _backing); // 5

const accessorDesc = Object.getOwnPropertyDescriptor(accessorObj, "computed");
console.log("accessor has get:", typeof accessorDesc?.get === "function"); // true
console.log("accessor has set:", typeof accessorDesc?.set === "function"); // true
console.log("accessor has value:", accessorDesc?.value); // undefined (accessor, not data)

console.log("All object gap tests complete.");

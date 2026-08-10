// #7542 + #7760: a replaced `Array.prototype[Symbol.iterator]` must drive every
// spread form, and the method must be a real own property of the prototype so
// the ordinary patch-and-restore idiom works.
//
// This test is only possible because of #7760: restoring the original by
// reference used to throw (`next is not a function` — the read returned a
// method bound to the PROTOTYPE), and leaving the slot patched takes the NODE
// oracle down after the script, since its own primordials build a `SafeMap`
// from an iterable and get the patched value.

const arrProto: any = Array.prototype;

// #7760: a real own property, like Map/Set/String/%TypedArray% already had.
const desc: any = Object.getOwnPropertyDescriptor(arrProto, Symbol.iterator);
console.log("descriptor:", typeof desc.value, desc.writable, desc.enumerable, desc.configurable);
console.log("hasOwn:", Object.prototype.hasOwnProperty.call(arrProto, Symbol.iterator));
console.log("in ownSymbols:", Object.getOwnPropertySymbols(arrProto).indexOf(Symbol.iterator) >= 0);
console.log("name:", desc.value.name);

const original = arrProto[Symbol.iterator];
// #7760: the value reads `this` at CALL time, so a borrowed reference works.
console.log("values.call:", JSON.stringify(Array.from(original.call([7, 8]) as any)));

arrProto[Symbol.iterator] = function* () {
  yield "patched";
};

const src = [1, 2, 3];
function count(...xs: any[]) {
  return xs.length;
}

// #7542: every spread form drives the patched method.
console.log("spread literal:", JSON.stringify([...[1, 2, 3]]));
console.log("spread variable:", JSON.stringify([...src]));
console.log("spread into array:", JSON.stringify([0, ...src, 9]));
console.log("call spread:", count(...src));
console.log("Array.from:", JSON.stringify(Array.from(src as any)));

// An OWN `[Symbol.iterator]` still wins over the prototype.
const own: any = [1, 2, 3];
own[Symbol.iterator] = function* () {
  yield "own";
};
console.log("own wins:", JSON.stringify([...own]));

// #7760: restore by reference, and by descriptor round-trip.
arrProto[Symbol.iterator] = original;
console.log("restored spread:", JSON.stringify([...src]));
console.log("restored Array.from:", JSON.stringify(Array.from(src as any)));
console.log("restored call spread:", count(...src));

Object.defineProperty(arrProto, Symbol.iterator, desc);
console.log("after defineProperty:", JSON.stringify([...src]));

// TypedArray callback methods return TypedArrays with element-typed storage.
// Index reads on the result must use the TypedArray accessor even when the
// static type has been erased by the callback-method lowering.

const mapped = new Uint16Array([1, 2]).map((value) => value * 2);
console.log("map instance:", mapped instanceof Uint16Array);
console.log("map indexes:", mapped[0], mapped[1]);
const mappedAny: any = mapped;
console.log("map any indexes:", mappedAny[0], mappedAny[1]);
console.log("map from:", Array.from(mapped).join("|"));

const filtered = new Int8Array([-1, 2, 3]).filter((value) => value > 0);
console.log("filter instance:", filtered instanceof Int8Array);
console.log("filter indexes:", filtered[0], filtered[1]);
const filteredAny: any = filtered;
console.log("filter any indexes:", filteredAny[0], filteredAny[1]);
console.log("filter from:", Array.from(filtered).join("|"));

const clampedAny: any = new Uint8ClampedArray([1, 2]);
clampedAny[0] = 300;
clampedAny[1] = -5;
console.log("dynamic set clamp:", clampedAny[0], clampedAny[1], Array.from(clampedAny).join("|"));

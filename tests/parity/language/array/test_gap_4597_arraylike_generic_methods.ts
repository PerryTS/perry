// #4597: Array.prototype methods generic over array-like receivers.
// Each Array.prototype method begins with O = ToObject(this); len =
// LengthOfArrayLike(O); then indexed Get/HasProperty on O. The explicit
// `Array.prototype.<m>.call(receiver, ...)` form must run the Array algorithm
// on a generic array-like receiver — a plain object, a function (length =
// arity, expando indices), a string (code-unit indices), or a primitive
// (boxed via ToObject) — preserving receiver identity for the callback's 3rd
// argument and honouring holes via HasProperty.

// --- plain array-like object ---
console.log(Array.prototype.map.call({ length: 3, 0: "a", 1: "b", 2: "c" }, (x: any, i: number) => x + i).join("|")); // a0|b1|c2
console.log(Array.prototype.filter.call({ length: 3, 0: 1, 2: 3 }, () => true).join(",")); // 1,3 (hole at 1 skipped)
console.log(Array.prototype.reduce.call({ length: 3, 0: 1, 1: 2, 2: 3 }, (s: any, x: any) => s + x, 10)); // 16
console.log(Array.prototype.indexOf.call({ length: 2, 0: "a", 1: "b" }, "b")); // 1
console.log(Array.prototype.join.call({ length: 2, 0: "a", 1: "b" }, "-")); // a-b

// --- callback observes the ORIGINAL receiver as its 3rd argument ---
(globalThis as any).Math.length = 1;
(globalThis as any).Math[0] = 1;
console.log(Array.prototype.map.call(Math, (_v: any, _i: any, obj: any) => Object.prototype.toString.call(obj))[0]); // [object Math]

// --- function receiver: length = arity, expando indices ---
function fn(a: any, b: any) {
  return a + b;
}
(fn as any)[1] = true;
console.log(Array.prototype.indexOf.call(fn, true)); // 1

// --- string receiver ---
console.log(Array.prototype.map.call("abc", (c: string) => c.toUpperCase()).join("")); // ABC
console.log(Array.prototype.indexOf.call("abc", "c")); // 2

// --- holes are skipped by iterators, preserved by map ---
console.log(Array.prototype.map.call({ length: 3, 0: "a", 2: "c" }, (x: any) => x + "!").join(",")); // a!,,c!

// --- thisArg binds the callback's `this` ---
console.log(
  Array.prototype.map
    .call([1, 2, 3], function (this: any, x: number) {
      return x * this.mul;
    }, { mul: 10 })
    .join(","),
); // 10,20,30

// --- primitive number receiver -> empty array-like ---
console.log(Array.prototype.map.call(5, (x: any) => x).length); // 0

// --- search / find family ---
console.log(Array.prototype.findLastIndex.call({ length: 3, 0: 1, 1: 2, 2: 3 }, (x: any) => x < 3)); // 1
console.log(Array.prototype.at.call({ length: 3, 0: "a", 1: "b", 2: "c" }, -1)); // c
console.log(Array.prototype.lastIndexOf.call([1, 2, 1, 2], 2)); // 3
console.log(Array.prototype.slice.call({ length: 4, 0: "a", 1: "b", 2: "c", 3: "d" }, -2).join(",")); // c,d

// --- nullish receiver throws TypeError ---
try {
  Array.prototype.forEach.call(null, () => {});
  console.log("no throw");
} catch (e: any) {
  console.log(e instanceof TypeError); // true
}

// --- non-callable callback throws TypeError ---
try {
  Array.prototype.map.call({ length: 1, 0: 1 }, undefined as any);
  console.log("no throw");
} catch (e: any) {
  console.log(e instanceof TypeError); // true
}

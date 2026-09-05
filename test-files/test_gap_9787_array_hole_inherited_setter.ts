// An in-bounds hole has no own property: it must consult inherited setters.
"use strict";

const calls: string[] = [];
Object.defineProperty(Array.prototype, "3", {
  configurable: true,
  get() { return "array-proto-three"; },
  set(this: any, value: any) { calls.push(`${Array.isArray(this)}:${value}`); },
});

try {
  const outOfBounds: any[] = [0];
  outOfBounds[3] = 31;
  console.log(calls.join(","), Object.hasOwn(outOfBounds, 3), outOfBounds[3], outOfBounds.length);

  const inBoundsHole: any[] = new Array(5);
  inBoundsHole[3] = 37;
  console.log(calls.join(","), Object.hasOwn(inBoundsHole, 3), inBoundsHole[3], inBoundsHole.length);

  // An own undefined value is not a hole and must bypass the inherited setter.
  const own: any[] = [0, 1, 2, undefined];
  own[3] = 41;
  console.log("own", calls.length, Object.hasOwn(own, 3), own[3], own.length);

  // Deletion creates the same obligation as new Array(n)'s initial holes.
  delete own[3];
  own[3] = 43;
  console.log("deleted", calls.join(","), Object.hasOwn(own, 3), own[3], own.length);
} finally {
  delete (Array.prototype as any)[3];
}

// The invalidation latch stays set after deletion, but no setter remains.
const plain: any[] = new Array(5);
plain[3] = 47;
console.log("removed", Object.hasOwn(plain, 3), plain[3], plain.length);

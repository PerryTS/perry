// An in-bounds hole has no own property: it must consult inherited setters.
// DEC 250 (Q2): a compiled program adds no index key to Array.prototype, so
// the inherited setter sits on the arrays' own prototype, defined after the
// arrays are linked to it.
"use strict";

const calls: string[] = [];
const proto: any = Object.create(Array.prototype);
const outOfBounds: any[] = [0];
const inBoundsHole: any[] = new Array(5);
const own: any[] = [0, 1, 2, undefined];
for (const a of [outOfBounds, inBoundsHole, own]) Object.setPrototypeOf(a, proto);
Object.defineProperty(proto, "3", {
  configurable: true,
  get() { return "array-proto-three"; },
  set(this: any, value: any) { calls.push(`${Array.isArray(this)}:${value}`); },
});

try {
  outOfBounds[3] = 31;
  console.log(calls.join(","), Object.hasOwn(outOfBounds, 3), outOfBounds[3], outOfBounds.length);

  inBoundsHole[3] = 37;
  console.log(calls.join(","), Object.hasOwn(inBoundsHole, 3), inBoundsHole[3], inBoundsHole.length);

  // An own undefined value is not a hole and must bypass the inherited setter.
  own[3] = 41;
  console.log("own", calls.length, Object.hasOwn(own, 3), own[3], own.length);

  // Deletion creates the same obligation as new Array(n)'s initial holes.
  delete own[3];
  own[3] = 43;
  console.log("deleted", calls.join(","), Object.hasOwn(own, 3), own[3], own.length);
} finally {
  delete proto[3];
}

// After the deletion no setter remains on the chain.
const plain: any[] = new Array(5);
Object.setPrototypeOf(plain, proto);
plain[3] = 47;
console.log("removed", Object.hasOwn(plain, 3), plain[3], plain.length);

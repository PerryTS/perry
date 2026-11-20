// Stress test: Cross-module linking
// Targets: exported functions, classes, constants, mutable vars, inheritance
// Based on bugs: module init order (#32), exported const vars, class method
// dispatch, ABI mismatch, cross-module return type refinement

import { PI, E, add, multiply, Vector, Point, counter, incrementCounter } from "./math_utils";

// === SECTION: Exported constants ===
console.log("PI:", PI);
console.log("E:", E);

// === SECTION: Exported functions ===
console.log("add(3, 4):", add(3, 4));
console.log("multiply(5, 6):", multiply(5, 6));
console.log("add(0, 0):", add(0, 0));
console.log("add(-1, 1):", add(-1, 1));

// === SECTION: Exported class instantiation ===
const v = new Vector(3, 4);
console.log("v.x:", v.x);
console.log("v.y:", v.y);
console.log("v.magnitude():", v.magnitude());
console.log("v.toString():", v.toString());

// === SECTION: Exported class with inheritance ===
const p = new Point(1, 2, "origin");
console.log("p.x:", p.x);
console.log("p.y:", p.y);
console.log("p.label:", p.label);
console.log("p.magnitude():", p.magnitude());
console.log("p.toString():", p.toString());

// === SECTION: instanceof across modules ===
console.log("v instanceof Vector:", v instanceof Vector);
console.log("p instanceof Vector:", p instanceof Vector);
console.log("p instanceof Point:", p instanceof Point);

// === SECTION: Exported mutable variable ===
console.log("counter initial:", counter);
console.log("increment:", incrementCounter());
console.log("increment:", incrementCounter());
console.log("increment:", incrementCounter());

// === SECTION: Using imported functions in expressions ===
const result = add(multiply(2, 3), multiply(4, 5));
console.log("2*3 + 4*5:", result);

// === SECTION: Array of imported class instances ===
const vectors: Vector[] = [];
for (let i = 0; i < 5; i++) {
  vectors.push(new Vector(i, i * 2));
}
for (let i = 0; i < vectors.length; i++) {
  console.log("v[" + i + "]:", vectors[i].toString());
}

// === SECTION: Imported class in object fields ===
const obj = { pos: new Vector(10, 20), name: "test" };
console.log("obj.pos.x:", obj.pos.x);
console.log("obj.pos.y:", obj.pos.y);
console.log("obj.name:", obj.name);

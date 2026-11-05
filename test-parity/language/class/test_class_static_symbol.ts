// Refs #420 / #618: Symbol-keyed static class fields.
// Drizzle's `static [entityKind] = "Table"` shape was the load-bearing
// site (consulted by `is(value, type)` chain).
const KIND = Symbol.for("test:kind");
class A {
    static [KIND] = "A-kind";
}
class B {
    static [KIND] = "B-kind";
}
console.log("A[KIND]:", (A as any)[KIND]);
console.log("B[KIND]:", (B as any)[KIND]);
console.log("KIND in A:", KIND in A);
console.log("KIND in B:", KIND in B);
console.log("eq:", (A as any)[KIND] === (B as any)[KIND]);
console.log("hasOwn:", Object.prototype.hasOwnProperty.call(A, KIND));

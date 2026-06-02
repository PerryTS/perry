// #1457: an object whose field is an array was inspected via the
// object-field formatter, which detected Errors by reading the value's first
// 32-bit word as an object_type. For an ArrayHeader that word is `length`, so
// a 2-element array field collided with OBJECT_TYPE_ERROR (2) and was misread
// as an Error — dereferencing element bits as string pointers and crashing.
// Inspect array fields of every short length plus nested / mixed shapes.
console.log({ a: [1] });
console.log({ a: [1, 2] });
console.log({ a: [1, 2, 3] });
console.log({ s: ["a", "b"] });
console.log({ nested: { items: [10, 20] } });
console.log({ mixed: [1, "two", true, null] });
console.log({ first: [1, 2], second: ["x", "y"] });
console.log([{ k: [1, 2] }, { k: [3, 4] }]);

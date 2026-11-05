// Object.assign mutates the target object and returns the same target on WASM.
const target = { a: 0, b: 0, c: "init" };
const source = { a: 1, b: 2, c: "hi" };
const returned = Object.assign(target, source);
console.log(target.a);
console.log(target.b);
console.log(target.c);
console.log(returned === target);

const merged = Object.assign({ a: 1 }, { b: 2 }, { a: 3, c: 4 });
console.log(merged.a);
console.log(merged.b);
console.log(merged.c);

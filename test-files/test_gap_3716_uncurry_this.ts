// #3716 — the "uncurry-this" idiom `Function.prototype.call.bind(method)`.
//
// Reading `.bind` *as a value* off the reified `Function.prototype.call`
// previously returned `undefined`, so the bound function was never created,
// and even when the `call.bind(...)` call form produced a function, the bound
// `this` (the target method) was never applied — the result was `undefined`.
//
// This is the exact shape test262's `harness/propertyHelper.js` uses:
//   var __hasOwnProperty = Function.prototype.call.bind(Object.prototype.hasOwnProperty);
//   var __join           = Function.prototype.call.bind(Array.prototype.join);
// so fixing it unblocks every `verifyProperty`-based positive case at once.

// Headline repro: hasOwnProperty via call-uncurry.
const hop = Function.prototype.call.bind(Object.prototype.hasOwnProperty);
console.log("hop({a:1}, 'a'):", hop({ a: 1 }, "a")); // true
console.log("hop({a:1}, 'b'):", hop({ a: 1 }, "b")); // false

// Array.prototype.join via call-uncurry (noop-backed proto method).
const join = Function.prototype.call.bind(Array.prototype.join);
console.log("join([1,2,3], '-'):", join([1, 2, 3], "-")); // 1-2-3

// Array.prototype.slice via call-uncurry (dedicated thunk).
const slice = Function.prototype.call.bind(Array.prototype.slice);
console.log("slice([1,2,3], 1):", slice([1, 2, 3], 1)); // [ 2, 3 ]

// Object.prototype.toString via call-uncurry.
const toStr = Function.prototype.call.bind(Object.prototype.toString);
console.log("toStr([]):", toStr([])); // [object Array]

// apply-uncurry: args supplied as an array.
const applyHop = Function.prototype.apply.bind(Object.prototype.hasOwnProperty);
console.log("applyHop({x:5}, ['x']):", applyHop({ x: 5 }, ["x"])); // true

// Reading `.bind` / `.call` / `.apply` off the reified call as values.
const call = Function.prototype.call;
console.log("typeof call:", typeof call); // function
console.log("typeof call.bind:", typeof call.bind); // function
const b = call.bind(Object.prototype.hasOwnProperty);
console.log("typeof b:", typeof b); // function
console.log("b({a:1}, 'a'):", b({ a: 1 }, "a")); // true

// Sanity: ordinary user-function `.bind` still works.
function f(this: any, x: number) {
    return this.v + x;
}
console.log("f.bind({v:10})(5):", f.bind({ v: 10 })(5)); // 15

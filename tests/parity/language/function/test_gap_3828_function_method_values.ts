// #3828 — Function.prototype.call/apply/bind work on function VALUES (dynamic
// dispatch + value reads), not just the statically-written
// `Array.prototype.slice.call(args)` shape.
//
// Two halves:
//   (1) reading `.call` / `.apply` / `.bind` off a function value yields a real
//       callable (so `typeof fn.apply === "function"` and the value can be
//       stored / passed / re-bound), and
//   (2) the builtin function-method values themselves
//       (`Function.prototype.call`, `Object.prototype.hasOwnProperty`, …) are
//       reified callables that actually perform their method when invoked
//       indirectly with runtime (non-literal) args.
//
// This is the call-bind / function-bind / hasown / get-intrinsic layer that
// sits under a large fraction of npm (Express, #3527).

function f(this: any, a: any, b: any) {
    return [this && this.tag, a, b];
}

// (1) `.call` / `.apply` / `.bind` read as a VALUE on a function.
console.log("typeof f.apply:", typeof (f as any).apply);
console.log("typeof f.call:", typeof (f as any).call);
console.log("typeof f.bind:", typeof (f as any).bind);

const tA = { tag: "A" };

// Direct `.apply` / `.call` with an object thisArg, runtime values.
console.log("f.apply:", JSON.stringify((f as any).apply(tA, [1, 2])));
console.log("f.call:", JSON.stringify((f as any).call(tA, 3, 4)));

// `.bind` returns a bound closure that itself runs the body.
const g: any = (f as any).bind(tA, 9);
console.log("typeof g:", typeof g, "g(8):", JSON.stringify(g(8)));

// (2) Function.prototype.call held in a var, then `.apply` on it with a
// runtime array — the captured-function-method-value shape.
const c: any = Function.prototype.call;
console.log("typeof c:", typeof c, "typeof c.apply:", typeof c.apply);
console.log("c.apply(f, ...):", JSON.stringify(c.apply(f, [tA, 5, 6])));

// Object.prototype.hasOwnProperty reified as a value + invoked via `.call`.
const hop: any = Object.prototype.hasOwnProperty;
console.log("typeof hop:", typeof hop);
const obj: any = { x: 1 };
console.log("hop.call(obj, 'x'):", hop.call(obj, "x"));
console.log("hop.call(obj, 'y'):", hop.call(obj, "y"));

// The standalone `hasown` pattern (function-bind + Function.prototype.call):
// `bind.call(call, $hasOwn)` returns a closure whose body does
// `target.apply(that, args.concat(...))` with target === Function.prototype.call.
function bind(this: any, that: any) {
    var target = this;
    var args = Array.prototype.slice.call(arguments, 1);
    return function (this: any) {
        return target.apply(that, args.concat(Array.prototype.slice.call(arguments)));
    };
}
var call: any = Function.prototype.call;
var $hasOwn: any = Object.prototype.hasOwnProperty;
var hasOwn: any = (bind as any).call(call, $hasOwn);
var o: any = { a: 1 };
console.log("hasOwn(o, 'a'):", hasOwn(o, "a"));
console.log("hasOwn(o, 'b'):", hasOwn(o, "b"));

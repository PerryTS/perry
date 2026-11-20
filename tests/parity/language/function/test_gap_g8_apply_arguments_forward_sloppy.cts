// G8 apply, sloppy half (CommonJS: non-strict functions). A function with no parameters maps
// none, so `f.apply(x, arguments)` may hand its argument list to the intrinsic apply; a replaced
// `apply` gets the real object, whose `callee` is the function. A function with simple
// parameters keeps the mapped object. Compared against Node.
const show = (...xs: any[]) => JSON.stringify(xs.map((x) => (x === undefined ? "u" : x)));
const sink: any = { m(...xs: any[]) { return show(...xs); } };
function noParams() { return sink.m.apply(sink, arguments); }
console.log((noParams as any)(1, 2), (noParams as any)());
const own: any = function () { return "own"; };
let got: any = null;
own.apply = function (_t: any, list: any) { got = list; return list.length; };
function toOwn() { return own.apply(null, arguments) + " " + (got.callee === toOwn) + " " + (got === arguments); }
console.log((toOwn as any)("a", "b", "c"));
// Mapped parameters: a write before the apply is seen through the object.
function mapped(a: any, b: any) { a = "w"; return sink.m.apply(sink, arguments) + b; }
console.log((mapped as any)(1, 2, 3));
function mappedOwn(a: any) { const r = own.apply(null, arguments); got[0] = "via-object"; return r + " " + a; }
console.log((mappedOwn as any)(1, 2));
